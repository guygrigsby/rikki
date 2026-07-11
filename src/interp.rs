use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;

use crate::ast::*;
use crate::value::{ClosureData, ErrVal, FnRef, MapKey, Value};

/// A variable slot, shared between a scope and any closures capturing it.
pub type Cell = Rc<RefCell<Value>>;

/// Program output: buffered for tests and run_source, streamed for the CLI
/// so interactive programs (input(), chat loops) work.
pub enum Out {
    Buf(String),
    Stdout,
}

impl Out {
    pub fn push_str(&mut self, s: &str) {
        match self {
            Out::Buf(b) => b.push_str(s),
            Out::Stdout => {
                use std::io::Write;
                print!("{s}");
                let _ = std::io::stdout().flush();
            }
        }
    }

    pub fn take(&mut self) -> String {
        match self {
            Out::Buf(b) => std::mem::take(b),
            Out::Stdout => String::new(),
        }
    }
}

#[derive(Debug)]
pub struct Fault {
    pub msg: String,
    pub stack: Vec<String>,
}

pub enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

/// Expression result: a value, an early function return triggered by
/// `check`, or a Python exception looking for its chain's handler.
pub enum Ev {
    V(Value),
    Ret(Value),
    PyErr(ErrVal),
}

macro_rules! val {
    ($e:expr) => {
        match $e? {
            Ev::V(v) => v,
            r => return Ok(r),
        }
    };
}

/// val! for callers returning Result<Flow, Fault>. Takes the interpreter so
/// the unhandled-py fault carries the nevla call stack (macro hygiene keeps
/// `self` out of the expansion).
macro_rules! sval {
    ($i:expr, $e:expr) => {
        match $e? {
            Ev::V(v) => v,
            Ev::Ret(r) => return Ok(Flow::Return(r)),
            // the checker forces py chains into check or destructure
            Ev::PyErr(e) => return Err($i.unhandled_py(e)),
        }
    };
}

pub struct Interp<'p> {
    fns: HashMap<String, &'p FnDecl>,
    pub(crate) structs: HashMap<String, Vec<(String, TypeExpr)>>,
    globals: HashMap<String, Value>,
    py_imports: Vec<String>,
    /// Scope slots are shared cells (ADR 0010): closures capture the cells
    /// of their free variables, so reads and writes flow both ways between
    /// a closure and its enclosing scope, Go's semantics.
    scopes: Vec<HashMap<String, Cell>>,
    saved: Vec<Vec<HashMap<String, Cell>>>,
    /// Return types of the active function, for zero-filling check returns.
    ret_stack: Vec<Vec<TypeExpr>>,
    call_stack: Vec<String>,
    /// (file of the executing fn, line of the executing statement),
    /// maintained for error origins (spec 10.1)
    pub(crate) cur_pos: (Option<String>, u32),
    pos_stack: Vec<(Option<String>, u32)>,
    pub out: Out,
    pub(crate) prog_args: Vec<String>,
    /// GPU cards this program holds via the gputex protocol, keyed by card
    /// id (stdlib/gpu.rs). Dropping a hold releases its flock.
    pub(crate) gpu_holds: HashMap<String, crate::stdlib::gpu::Held>,
}

impl<'p> Interp<'p> {
    pub fn new(prog: &'p Program) -> Self {
        let mut fns = HashMap::new();
        let mut structs = HashMap::new();
        let mut globals = HashMap::new();
        let mut py_imports = vec![];
        for d in &prog.decls {
            match d {
                Decl::Fn(f) => {
                    fns.insert(f.name.clone(), f);
                }
                Decl::Struct { name, fields, .. } => {
                    structs.insert(name.clone(), fields.clone());
                }
                Decl::Import { path, py, .. } => {
                    if !*py {
                        globals.insert(path.clone(), Value::Module(path.clone()));
                        if path == "http" {
                            structs.extend(crate::stdlib::http::struct_exprs());
                        }
                        if path == "time" {
                            structs.extend(crate::stdlib::time::struct_exprs());
                        }
                        if path == "regex" {
                            structs.extend(crate::stdlib::regex::struct_exprs());
                        }
                        if path == "flag" {
                            structs.extend(crate::stdlib::flag::struct_exprs());
                        }
                    }
                    if *py {
                        py_imports.push(path.clone());
                    }
                }
            }
        }
        Interp {
            fns,
            structs,
            globals,
            py_imports,
            scopes: vec![],
            saved: vec![],
            ret_stack: vec![],
            call_stack: vec![],
            cur_pos: (None, 0),
            pos_stack: vec![],
            out: Out::Buf(String::new()),
            prog_args: vec![],
            gpu_holds: HashMap::new(),
        }
    }

    /// "file:line" (or "line N") of the executing statement, for error
    /// origins (spec 10.1).
    pub(crate) fn origin(&self) -> String {
        match &self.cur_pos {
            (Some(f), l) => format!("{f}:{l}"),
            (None, l) => format!("line {l}"),
        }
    }

    /// Stamp an origin onto a bridge error the moment it enters value
    /// space, once (spec 10.1).
    fn stamped(&self, mut e: ErrVal) -> ErrVal {
        if e.origin.is_empty() {
            e.origin = self.origin();
        }
        e
    }

    pub(crate) fn fault(&self, msg: impl Into<String>) -> Fault {
        Fault {
            msg: msg.into(),
            stack: self.call_stack.clone(),
        }
    }

    /// A python error escaping its chain unconsumed; carries the nevla stack.
    fn unhandled_py(&self, e: ErrVal) -> Fault {
        self.fault(format!("unhandled python error: {}", e.msg))
    }

    /// Bind a name in the innermost scope. Every path into user code seeds
    /// the stack (enter, repl_init); an empty stack here is an interpreter
    /// bug, reported as a fault rather than a panic.
    fn bind(&mut self, name: String, v: Value) -> Result<(), Fault> {
        match self.scopes.last_mut() {
            Some(s) => {
                s.insert(name, Rc::new(RefCell::new(v)));
                Ok(())
            }
            None => Err(self.fault("internal: no scope to bind into")),
        }
    }

    /// The cell holding `name`, innermost scope first.
    fn lookup_cell(&self, n: &str) -> Option<Cell> {
        for s in self.scopes.iter().rev() {
            if let Some(c) = s.get(n) {
                return Some(Rc::clone(c));
            }
        }
        None
    }

    /// Run fn main. Returns main's error value if it returned one.
    pub fn run_main(&mut self) -> Result<Option<ErrVal>, Fault> {
        self.run_named("main")
    }

    /// Import py modules, then call one nullary function by name: main, or
    /// a Test function under `nevla test`.
    pub fn run_named(&mut self, entry: &str) -> Result<Option<ErrVal>, Fault> {
        for m in self.py_imports.clone() {
            // import the full dotted path (loading the submodule), then bind
            // the top segment, Python's own semantics (spec 13.1)
            if let Err(e) = crate::bridge::import(&m) {
                return Err(self.fault(format!("import py \"{m}\": {}", e.msg)));
            }
            let top = m.split('.').next().unwrap_or(&m).to_string();
            match crate::bridge::import(&top) {
                Ok(h) => {
                    self.globals.insert(top, Value::Py(h));
                }
                Err(e) => return Err(self.fault(format!("import py \"{m}\": {}", e.msg))),
            }
        }
        let v = self.call_named(entry, vec![])?;
        Ok(match v {
            Value::Err(e) => Some(e),
            _ => None,
        })
    }

    /// Nevla call-stack depth cap: past this the call faults instead of
    /// overflowing the Rust stack (each nevla frame is several Rust
    /// frames deep through eval, so the cap must leave real-stack headroom
    /// even in debug builds).
    const RECURSION_LIMIT: usize = 1000;

    fn depth_check(&self) -> Result<(), Fault> {
        if self.call_stack.len() < Self::RECURSION_LIMIT {
            return Ok(());
        }
        // truncated stack: outermost few, a marker, innermost few
        let n = self.call_stack.len();
        let stack = if n > 30 {
            let mut s: Vec<String> = self.call_stack[..5].to_vec();
            s.push(format!("... ({} frames elided)", n - 30));
            s.extend(self.call_stack[n - 25..].iter().cloned());
            s
        } else {
            self.call_stack.clone()
        };
        Err(Fault {
            msg: format!("recursion limit exceeded ({})", Self::RECURSION_LIMIT),
            stack,
        })
    }

    fn call_named(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
        self.depth_check()?;
        let Some(f) = self.fns.get(name).copied() else {
            return Err(self.fault(format!("unknown function: {name}")));
        };
        if args.len() != f.params.len() {
            return Err(self.fault(format!("{name}: wrong argument count")));
        }
        let mut scope = HashMap::new();
        for (p, a) in f.params.iter().zip(args) {
            scope.insert(p.name.clone(), Rc::new(RefCell::new(a)));
        }
        let file = f.file.clone();
        self.enter(name.to_string(), vec![scope], f.ret.clone());
        self.cur_pos.0 = file;
        let flow = self.exec_block_no_scope(&f.body);
        self.leave();
        match flow? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Unit),
        }
    }

    fn call_closure(&mut self, c: &ClosureData, args: Vec<Value>) -> Result<Value, Fault> {
        self.depth_check()?;
        if args.len() != c.params.len() {
            return Err(self.fault("function value: wrong argument count"));
        }
        let mut scope = HashMap::new();
        for (p, a) in c.params.iter().zip(args) {
            scope.insert(p.name.clone(), Rc::new(RefCell::new(a)));
        }
        self.enter("fn".into(), vec![c.captured.clone(), scope], c.ret.clone());
        // expression body: a lone expression statement yields its value
        let flow = if c.body.len() == 1 {
            if let StmtKind::Expr(e) = &c.body[0].kind {
                match self.eval(e) {
                    Ok(Ev::V(v)) | Ok(Ev::Ret(v)) => Ok(Flow::Return(v)),
                    Ok(Ev::PyErr(e)) => Err(self.unhandled_py(e)),
                    Err(f) => Err(f),
                }
            } else {
                self.exec_block_no_scope(&c.body)
            }
        } else {
            self.exec_block_no_scope(&c.body)
        };
        self.leave();
        match flow? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Unit),
        }
    }

    pub(crate) fn has_fn(&self, name: &str) -> bool {
        self.fns.contains_key(name)
    }

    pub(crate) fn call_fn_by_name(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
        self.call_named(name, args)
    }

    pub(crate) fn call_value(&mut self, f: &Value, args: Vec<Value>) -> Result<Value, Fault> {
        match f {
            Value::Fn(FnRef::Decl(name)) => self.call_named(&name.clone(), args),
            Value::Fn(FnRef::Closure(c)) => self.call_closure(&Rc::clone(c), args),
            Value::Fn(FnRef::Zero) => Err(self.fault("called the zero value of a function type")),
            _ => Err(self.fault("not callable")),
        }
    }

    fn enter(&mut self, name: String, scopes: Vec<HashMap<String, Cell>>, ret: Vec<TypeExpr>) {
        self.call_stack.push(name);
        self.ret_stack.push(ret);
        self.saved.push(std::mem::replace(&mut self.scopes, scopes));
        // heap-side, not a Rust-frame local: the recursion golden rides
        // the interpreter thread's stack budget closely
        self.pos_stack.push(self.cur_pos.clone());
    }

    fn leave(&mut self) {
        if let Some(p) = self.pos_stack.pop() {
            self.cur_pos = p;
        }
        self.scopes = self.saved.pop().unwrap_or_default();
        self.ret_stack.pop();
        self.call_stack.pop();
    }

    // ---------- repl support ----------

    /// One persistent top-level scope for repl bindings.
    pub fn repl_init(&mut self) {
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }
    }

    /// Register a declaration parsed at the repl. Py imports resolve now.
    pub fn repl_decl(&mut self, d: &'p Decl) -> Result<(), Fault> {
        match d {
            Decl::Fn(f) => {
                self.fns.insert(f.name.clone(), f);
            }
            Decl::Struct { name, fields, .. } => {
                self.structs.insert(name.clone(), fields.clone());
            }
            Decl::Import { path, py, .. } => {
                if *py {
                    match crate::bridge::import(path) {
                        Ok(h) => {
                            self.globals.insert(path.clone(), Value::Py(h));
                        }
                        Err(e) => return Err(self.fault(format!("import py {path:?}: {}", e.msg))),
                    }
                } else {
                    self.globals
                        .insert(path.clone(), Value::Module(path.clone()));
                }
            }
        }
        Ok(())
    }

    /// Execute one repl statement; expression statements yield their value.
    pub fn repl_stmt(&mut self, s: &'p Stmt) -> Result<Option<Value>, Fault> {
        self.repl_init();
        if let StmtKind::Expr(e) = &s.kind {
            return match self.eval(e)? {
                Ev::V(v) | Ev::Ret(v) => Ok(match v {
                    Value::Unit => None,
                    v => Some(v),
                }),
                Ev::PyErr(e) => Ok(Some(Value::Err(e))),
            };
        }
        match self.exec_stmt(s)? {
            Flow::Return(v) if !matches!(v, Value::Unit) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    pub fn take_out(&mut self) -> String {
        self.out.take()
    }

    pub fn set_args(&mut self, args: Vec<String>) {
        self.prog_args = args;
    }

    pub fn stream_stdout(&mut self) {
        self.out = Out::Stdout;
    }

    // ---------- statements ----------

    fn exec_block(&mut self, b: &Block) -> Result<Flow, Fault> {
        self.scopes.push(HashMap::new());
        let r = self.exec_block_no_scope(b);
        self.scopes.pop();
        r
    }

    fn exec_block_no_scope(&mut self, b: &Block) -> Result<Flow, Fault> {
        for s in b {
            match self.exec_stmt(s)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, s: &Stmt) -> Result<Flow, Fault> {
        self.cur_pos.1 = s.span.line;
        match &s.kind {
            StmtKind::Let { names, expr } => {
                let v = match self.eval(expr)? {
                    Ev::V(v) => v,
                    Ev::Ret(r) => return Ok(Flow::Return(r)),
                    Ev::PyErr(e) => {
                        // py chain destructured: zero-fill values, bind error;
                        // the value slots of a py chain are always py-typed,
                        // so their zero is a Python None handle
                        let mut parts = vec![
                            Value::Py(crate::bridge::py_none());
                            names.len().saturating_sub(1)
                        ];
                        parts.push(Value::Err(e));
                        for (n, p) in names.iter().zip(parts) {
                            if n != "_" {
                                self.bind(n.clone(), p)?;
                            }
                        }
                        return Ok(Flow::Normal);
                    }
                };
                let parts = if names.len() > 1 {
                    match v {
                        Value::Tuple(ts) => ts,
                        // a successful py chain is one value plus an empty
                        // error slot
                        one @ Value::Py(_) if names.len() == 2 => vec![one, Value::NoneV],
                        one => vec![one],
                    }
                } else {
                    // single name binds the whole value (tuples can't reach
                    // here; the checker rejects them)
                    vec![v]
                };
                if parts.len() != names.len() {
                    return Err(self.fault("destructure arity mismatch"));
                }
                for (n, p) in names.iter().zip(parts) {
                    if n != "_" {
                        self.bind(n.clone(), p)?;
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Assign { target, expr } => {
                let v = sval!(self, self.eval(expr));
                match self.assign(target, v)? {
                    Ev::Ret(r) => Ok(Flow::Return(r)),
                    Ev::PyErr(e) => Err(self.unhandled_py(e)),
                    Ev::V(_) => Ok(Flow::Normal),
                }
            }
            StmtKind::Expr(e) => {
                sval!(self, self.eval(e));
                Ok(Flow::Normal)
            }
            StmtKind::Return(exprs) => {
                let mut vals = vec![];
                for e in exprs {
                    vals.push(sval!(self, self.eval(e)));
                }
                let v = match vals.len() {
                    0 => Value::Unit,
                    1 => vals.into_iter().next().unwrap(),
                    _ => Value::Tuple(vals),
                };
                Ok(Flow::Return(v))
            }
            StmtKind::If {
                cond,
                then,
                elifs,
                els,
            } => {
                if truthy(&sval!(self, self.eval(cond))) {
                    return self.exec_block(then);
                }
                for (c, b) in elifs {
                    if truthy(&sval!(self, self.eval(c))) {
                        return self.exec_block(b);
                    }
                }
                if let Some(b) = els {
                    return self.exec_block(b);
                }
                Ok(Flow::Normal)
            }
            StmtKind::ForRange { names, iter, body } => {
                let it = sval!(self, self.eval(iter));
                if let Value::Py(h) = &it {
                    // out of line: exec_stmt is on every recursive path and
                    // this block's locals would tax the whole call stack
                    let h = h.clone();
                    return self.py_range(names, &h, body);
                }
                if let Value::List(items) = &it {
                    let items = items.clone();
                    return self.list_range(names, &items, body);
                }
                if let Value::Map(m) = &it {
                    let m = m.clone();
                    return self.map_range(names, &m, body);
                }
                let rounds: Box<dyn Iterator<Item = Vec<Value>>> =
                    match it {
                        Value::Int(n) => Box::new((0..n.max(0)).map(|i| vec![Value::Int(i)])),
                        Value::Str(s) => {
                            let chars: Vec<char> = s.chars().collect();
                            Box::new(chars.into_iter().enumerate().map(|(i, c)| {
                                vec![Value::Int(i as i64), Value::Str(c.to_string())]
                            }))
                        }
                        _ => return Err(self.fault("cannot range over this value")),
                    };
                for round in rounds {
                    self.scopes.push(HashMap::new());
                    for (n, v) in names.iter().zip(round) {
                        if n != "_" {
                            self.bind(n.clone(), v)?;
                        }
                    }
                    let flow = self.exec_block_no_scope(body);
                    self.scopes.pop();
                    match flow? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        r @ Flow::Return(_) => return Ok(r),
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::ForCond { cond, body } => {
                loop {
                    if let Some(c) = cond {
                        if !truthy(&sval!(self, self.eval(c))) {
                            break;
                        }
                    }
                    match self.exec_block(body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        r @ Flow::Return(_) => return Ok(r),
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::With { expr, body } => {
                let v = sval!(self, self.eval(expr));
                let Value::Py(h) = v else {
                    return Err(self.fault("with needs a py value"));
                };
                self.py_with(&h, body)
            }
            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Continue => Ok(Flow::Continue),
        }
    }

    /// `for range` over a list: the length is fixed at loop entry, element
    /// writes during iteration are visible, growth is not visited (append
    /// is pure, so growth is a different list anyway).
    #[inline(never)]
    fn list_range(
        &mut self,
        names: &[String],
        items: &crate::value::ListRef,
        body: &Block,
    ) -> Result<Flow, Fault> {
        let n = items.borrow().len();
        for i in 0..n {
            let Some(v) = items.borrow().get(i).cloned() else {
                break;
            };
            self.scopes.push(HashMap::new());
            for (name, val) in names.iter().zip([Value::Int(i as i64), v]) {
                if name != "_" {
                    self.bind(name.clone(), val)?;
                }
            }
            let flow = self.exec_block_no_scope(body);
            self.scopes.pop();
            match flow? {
                Flow::Normal | Flow::Continue => {}
                Flow::Break => break,
                r @ Flow::Return(_) => return Ok(r),
            }
        }
        Ok(Flow::Normal)
    }

    /// `for range` over a map: keys are snapshotted at loop entry; entries
    /// deleted mid-iteration are skipped, entries added are not visited.
    #[inline(never)]
    fn map_range(
        &mut self,
        names: &[String],
        m: &crate::value::MapRef,
        body: &Block,
    ) -> Result<Flow, Fault> {
        let keys: Vec<MapKey> = m.borrow().keys().cloned().collect();
        for k in keys {
            let Some(v) = m.borrow().get(&k).cloned() else {
                continue;
            };
            self.scopes.push(HashMap::new());
            for (name, val) in names.iter().zip([k.to_value(), v]) {
                if name != "_" {
                    self.bind(name.clone(), val)?;
                }
            }
            let flow = self.exec_block_no_scope(body);
            self.scopes.pop();
            match flow? {
                Flow::Normal | Flow::Continue => {}
                Flow::Break => break,
                r @ Flow::Return(_) => return Ok(r),
            }
        }
        Ok(Flow::Normal)
    }

    /// `for range` over a py iterable: iter() once, __next__ per round,
    /// StopIteration ends the loop, anything else faults.
    #[inline(never)]
    fn py_range(
        &mut self,
        names: &[String],
        h: &crate::bridge::PyHandle,
        body: &Block,
    ) -> Result<Flow, Fault> {
        let pit = crate::bridge::iter(h).map_err(|e| self.fault(format!("py range: {}", e.msg)))?;
        let mut i: i64 = 0;
        loop {
            let item = match crate::bridge::next(&pit) {
                Ok(Some(v)) => v,
                Ok(None) => break,
                Err(e) => return Err(self.fault(format!("py range: {}", e.msg))),
            };
            self.scopes.push(HashMap::new());
            for (n, v) in names.iter().zip([Value::Int(i), item]) {
                if n != "_" {
                    self.bind(n.clone(), v)?;
                }
            }
            let flow = self.exec_block_no_scope(body);
            self.scopes.pop();
            match flow? {
                Flow::Normal | Flow::Continue => {}
                Flow::Break => break,
                r @ Flow::Return(_) => return Ok(r),
            }
            i += 1;
        }
        Ok(Flow::Normal)
    }

    /// `with`: __enter__, body, __exit__ on every nevla-level exit. A return
    /// carrying an error becomes a synthesized exception so exits that branch
    /// on exception info (transactions) behave; a fault skips __exit__
    /// entirely (spec 8.9). Exceptions from enter/exit fault.
    #[inline(never)]
    fn py_with(&mut self, h: &crate::bridge::PyHandle, body: &Block) -> Result<Flow, Fault> {
        crate::bridge::enter(h).map_err(|e| self.fault(format!("py with: {}", e.msg)))?;
        let flow = self.exec_block(body)?;
        let err = match &flow {
            Flow::Return(v) => match v {
                Value::Err(e) => Some(e),
                Value::Tuple(vs) => match vs.last() {
                    Some(Value::Err(e)) => Some(e),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        let had_err = err.is_some();
        let suppressed =
            crate::bridge::exit(h, err).map_err(|e| self.fault(format!("py with: {}", e.msg)))?;
        if suppressed && had_err {
            // nevla control flow cannot be resurrected by a py call; loud
            // beats silently continuing with zeroed results
            return Err(self.fault("py with: __exit__ cannot suppress a nevla error"));
        }
        Ok(flow)
    }

    // ---------- assignment ----------

    fn assign(&mut self, target: &Expr, v: Value) -> Result<Ev, Fault> {
        // collect the path down to a base identifier
        enum Step {
            Idx(Value),
            Field(String),
        }
        fn py_assign(
            h: &crate::bridge::PyHandle,
            steps: Vec<Step>,
            v: Value,
        ) -> Result<(), String> {
            let mut cur = h.clone();
            let n = steps.len();
            for (i, st) in steps.into_iter().enumerate() {
                if i + 1 == n {
                    let r = match st {
                        Step::Field(f) => crate::bridge::setattr(&cur, &f, &v),
                        Step::Idx(k) => crate::bridge::setitem(&cur, &k, &v),
                    };
                    return r.map_err(|e| format!("py assignment: {}", e.msg));
                }
                let next = match st {
                    Step::Field(f) => crate::bridge::getattr(&cur, &f),
                    Step::Idx(k) => crate::bridge::index(&cur, &k),
                }
                .map_err(|e| format!("py assignment: {}", e.msg))?;
                match next {
                    Value::Py(nh) => cur = nh,
                    _ => return Err("py assignment: chain left python".into()),
                }
            }
            // steps are nonempty by construction; the loop always returns
            Err("py assignment: empty path".into())
        }
        // Navigate &mut within one value (structs are inline); delegate to a
        // container or the bridge at the first reference boundary. Deeper
        // container hops recurse, each holding at most one borrow; a path
        // that aliases itself is a clean error, never a RefCell abort.
        fn assign_into(mut slot: &mut Value, steps: Vec<Step>, v: Value) -> Result<(), String> {
            let mut steps = steps.into_iter().peekable();
            loop {
                if steps.peek().is_none() {
                    *slot = v;
                    return Ok(());
                }
                match slot {
                    Value::Py(h) => return py_assign(h, steps.collect(), v),
                    Value::List(items) => {
                        let items = items.clone();
                        return assign_list(&items, steps, v);
                    }
                    Value::Map(m) => {
                        let m = m.clone();
                        return assign_map(&m, steps, v);
                    }
                    _ => {}
                }
                let Some(st) = steps.next() else {
                    return Err("internal: assignment path underflow".into());
                };
                match st {
                    Step::Field(f) => match slot {
                        Value::Struct { fields, .. } => match fields.get_mut(&f) {
                            Some(x) => slot = x,
                            None => return Err("unknown field".into()),
                        },
                        _ => return Err("cannot assign field here".into()),
                    },
                    Step::Idx(_) => match slot {
                        Value::Str(_) => return Err("cannot assign into a string".into()),
                        _ => return Err("cannot index this value".into()),
                    },
                }
            }
        }
        fn assign_list(
            items: &crate::value::ListRef,
            mut steps: std::iter::Peekable<std::vec::IntoIter<Step>>,
            v: Value,
        ) -> Result<(), String> {
            let idx = match steps.next() {
                Some(Step::Idx(idx)) => idx,
                Some(Step::Field(_)) => return Err("cannot assign field here".into()),
                None => return Err("internal: assignment path underflow".into()),
            };
            let i = match idx {
                Value::Int(i) => i,
                _ => return Err("index must be int".into()),
            };
            let Ok(mut b) = items.try_borrow_mut() else {
                return Err("assignment path aliases itself".into());
            };
            let len = b.len() as i64;
            if i < 0 || i >= len {
                return Err(format!("index out of bounds: {i} of {len}"));
            }
            if steps.peek().is_none() {
                b[i as usize] = v;
                return Ok(());
            }
            assign_into(&mut b[i as usize], steps.collect(), v)
        }
        fn assign_map(
            m: &crate::value::MapRef,
            mut steps: std::iter::Peekable<std::vec::IntoIter<Step>>,
            v: Value,
        ) -> Result<(), String> {
            let idx = match steps.next() {
                Some(Step::Idx(idx)) => idx,
                Some(Step::Field(_)) => return Err("cannot assign field here".into()),
                None => return Err("internal: assignment path underflow".into()),
            };
            let Some(k) = MapKey::from_value(&idx) else {
                return Err("bad map key".into());
            };
            let Ok(mut b) = m.try_borrow_mut() else {
                return Err("assignment path aliases itself".into());
            };
            if steps.peek().is_none() {
                b.insert(k, v);
                return Ok(());
            }
            match b.get_mut(&k) {
                Some(x) => assign_into(x, steps.collect(), v),
                None => Err("missing key".into()),
            }
        }
        let mut steps = vec![];
        let mut cur = target;
        loop {
            match &cur.kind {
                ExprKind::Ident(name) => {
                    let name = name.clone();
                    steps.reverse();
                    // locate the variable's cell
                    let Some(cell) = self.lookup_cell(&name) else {
                        // py imports live in globals; the name itself is not
                        // assignable but its referent is, through the handle
                        if !steps.is_empty() {
                            if let Some(Value::Py(h)) = self.globals.get(&name) {
                                let h = h.clone();
                                return match py_assign(&h, steps, v) {
                                    Ok(()) => Ok(Ev::V(Value::Unit)),
                                    Err(msg) => Err(self.fault(msg)),
                                };
                            }
                        }
                        return Err(self.fault(format!("undefined: {name}")));
                    };
                    // descend; errors come back as messages so the nevla
                    // stack is cloned only on the failure path. The cell
                    // borrow is safe: steps and value are already evaluated,
                    // nothing below re-enters the evaluator.
                    let Ok(mut slot) = cell.try_borrow_mut() else {
                        return Err(self.fault("internal: variable cell aliased during assignment"));
                    };
                    match assign_into(&mut slot, steps, v) {
                        Ok(()) => return Ok(Ev::V(Value::Unit)),
                        Err(msg) => return Err(self.fault(msg)),
                    }
                }
                ExprKind::Index { recv, idx } => {
                    let i = val!(self.eval(idx));
                    steps.push(Step::Idx(i));
                    cur = recv;
                }
                ExprKind::Field { recv, name } => {
                    steps.push(Step::Field(name.clone()));
                    cur = recv;
                }
                _ => return Err(self.fault("cannot assign to this expression")),
            }
        }
    }

    // ---------- expressions ----------

    pub(crate) fn eval(&mut self, e: &Expr) -> Result<Ev, Fault> {
        use ExprKind as K;
        let ok = |v| Ok(Ev::V(v));
        match &e.kind {
            K::Int(v) => ok(Value::Int(*v)),
            K::Float(v) => ok(Value::Float(*v)),
            K::Str(s) => ok(Value::Str(s.clone())),
            K::Bool(b) => ok(Value::Bool(*b)),
            K::NoneLit => ok(Value::NoneV),
            K::Ident(n) => {
                if let Some(c) = self.lookup_cell(n) {
                    let v = c.borrow().clone();
                    return ok(v);
                }
                if self.fns.contains_key(n) {
                    return ok(Value::Fn(FnRef::Decl(n.clone())));
                }
                if let Some(v) = self.globals.get(n) {
                    return ok(v.clone());
                }
                Err(self.fault(format!("undefined: {n}")))
            }
            K::List(items) | K::ListLit { items, .. } => {
                let mut out = vec![];
                for it in items {
                    out.push(val!(self.eval(it)));
                }
                ok(Value::list(out))
            }
            K::MapLit { entries, .. } => {
                let mut m = IndexMap::new();
                for (k, v) in entries {
                    let kv = val!(self.eval(k));
                    let Some(key) = MapKey::from_value(&kv) else {
                        return Err(self.fault("bad map key"));
                    };
                    let vv = val!(self.eval(v));
                    m.insert(key, vv);
                }
                ok(Value::map(m))
            }
            K::StructLit { name, fields } => {
                if !self.structs.contains_key(name) {
                    return Err(self.fault(format!("unknown struct: {name}")));
                }
                let mut vals = HashMap::new();
                for (f, v) in fields {
                    vals.insert(f.clone(), val!(self.eval(v)));
                }
                // field order follows the declaration; def borrowed, not
                // cloned, per literal
                let Some(def) = self.structs.get(name) else {
                    return Err(self.fault(format!("unknown struct: {name}")));
                };
                let mut out = IndexMap::new();
                for (f, _) in def {
                    match vals.remove(f) {
                        Some(v) => {
                            out.insert(f.clone(), v);
                        }
                        None => return Err(self.fault(format!("missing field: {f}"))),
                    }
                }
                ok(Value::Struct {
                    name: name.clone(),
                    fields: out,
                })
            }
            K::Unary { op, rhs } => {
                let v = val!(self.eval(rhs));
                match (op, v) {
                    (UnOp::Not, Value::Bool(b)) => ok(Value::Bool(!b)),
                    (UnOp::Neg, Value::Int(i)) => match i.checked_neg() {
                        Some(n) => ok(Value::Int(n)),
                        None => Err(self.fault("integer overflow")),
                    },
                    (UnOp::Neg, Value::Float(f)) => ok(Value::Float(-f)),
                    _ => Err(self.fault("bad operand")),
                }
            }
            K::Binary { op, lhs, rhs } => {
                // short-circuit
                if matches!(op, BinOp::And | BinOp::Or) {
                    let l = truthy(&val!(self.eval(lhs)));
                    let r = match (op, l) {
                        (BinOp::And, false) => false,
                        (BinOp::Or, true) => true,
                        _ => truthy(&val!(self.eval(rhs))),
                    };
                    return ok(Value::Bool(r));
                }
                let l = val!(self.eval(lhs));
                let r = val!(self.eval(rhs));
                if matches!(l, Value::Py(_)) || matches!(r, Value::Py(_)) {
                    return Ok(match crate::bridge::binop(*op, &l, &r) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(self.stamped(e)),
                    });
                }
                self.binop(*op, l, r).map(Ev::V)
            }
            K::Call {
                callee,
                args,
                kwargs,
            } => {
                let mut vals = vec![];
                // builtins by bare name, unless shadowed
                if let K::Ident(name) = &callee.kind {
                    let shadowed = self.scopes.iter().rev().any(|s| s.contains_key(name))
                        || self.fns.contains_key(name);
                    if !shadowed {
                        for a in args {
                            vals.push(val!(self.eval(a)));
                        }
                        return self.builtin_call(name, vals).map(Ev::V);
                    }
                }
                let f = val!(self.eval(callee));
                for a in args {
                    vals.push(val!(self.eval(a)));
                }
                if let Value::Py(h) = &f {
                    let mut kws = vec![];
                    for (k, v) in kwargs {
                        kws.push((k.clone(), val!(self.eval(v))));
                    }
                    return Ok(match crate::bridge::call(h, &vals, &kws) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(self.stamped(e)),
                    });
                }
                if !kwargs.is_empty() {
                    return Err(self.fault("named arguments are only for python calls"));
                }
                self.call_value(&f, vals).map(Ev::V)
            }
            K::Method {
                recv,
                name,
                args,
                kwargs,
            } => {
                // error.new / error.wrap
                if let K::Ident(id) = &recv.kind {
                    let shadowed = self.scopes.iter().rev().any(|s| s.contains_key(id));
                    if id == "error" && !shadowed {
                        let mut vals = vec![];
                        for a in args {
                            vals.push(val!(self.eval(a)));
                        }
                        return self.error_builtin(name, vals).map(Ev::V);
                    }
                }
                let r = val!(self.eval(recv));
                let mut vals = vec![];
                for a in args {
                    vals.push(val!(self.eval(a)));
                }
                if let Value::Py(h) = &r {
                    let f = match crate::bridge::getattr(h, name) {
                        Ok(v) => v,
                        Err(e) => return Ok(Ev::PyErr(self.stamped(e))),
                    };
                    let Value::Py(fh) = &f else { unreachable!() };
                    let mut kws = vec![];
                    for (k, v) in kwargs {
                        kws.push((k.clone(), val!(self.eval(v))));
                    }
                    return Ok(match crate::bridge::call(fh, &vals, &kws) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(self.stamped(e)),
                    });
                }
                if !kwargs.is_empty() {
                    return Err(self.fault("named arguments are only for python calls"));
                }
                self.method_call(r, name, vals).map(Ev::V)
            }
            K::Field { recv, name } => {
                let r = val!(self.eval(recv));
                if let Value::Py(h) = &r {
                    return Ok(match crate::bridge::getattr(h, name) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(self.stamped(e)),
                    });
                }
                self.field(&r, name).map(Ev::V)
            }
            K::Index { recv, idx } => {
                let r = val!(self.eval(recv));
                let i = val!(self.eval(idx));
                if let Value::Py(h) = &r {
                    return Ok(match crate::bridge::index(h, &i) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(self.stamped(e)),
                    });
                }
                self.index(&r, i).map(Ev::V)
            }
            K::Slice { recv, lo, hi } => {
                let r = val!(self.eval(recv));
                let lo = val!(self.eval(lo));
                let hi = val!(self.eval(hi));
                self.slice(&r, lo, hi).map(Ev::V)
            }
            K::Lambda { params, ret, body } => {
                // capture the cells of the body's free variables: closure
                // and enclosing scope share them (ADR 0010, Go semantics)
                let mut captured = HashMap::new();
                for name in free_vars(params, body) {
                    if let Some(cell) = self.lookup_cell(&name) {
                        captured.insert(name, cell);
                    }
                }
                ok(Value::Fn(FnRef::Closure(Rc::new(ClosureData {
                    params: params.clone(),
                    ret: ret.clone().unwrap_or_default(),
                    body: body.clone(),
                    captured,
                }))))
            }
            K::Check(inner) => {
                let v = match self.eval(inner)? {
                    Ev::V(v) => v,
                    r @ Ev::Ret(_) => return Ok(r),
                    Ev::PyErr(e) => Value::Tuple(vec![Value::NoneV, Value::Err(e)]),
                };
                let mut parts = match v {
                    Value::Tuple(ts) => ts,
                    // fn returning a lone error?: the value IS the error slot
                    one @ (Value::Err(_) | Value::NoneV) => vec![one],
                    // successful py chain: the value with an implicit empty
                    // error slot
                    other => vec![other, Value::NoneV],
                };
                let err_slot = parts.pop().unwrap_or(Value::Unit);
                match err_slot {
                    Value::Err(e) => {
                        // zero-fill the enclosing function's other returns
                        let rets = self.ret_stack.last().cloned().unwrap_or_default();
                        let mut out: Vec<Value> = rets
                            .iter()
                            .take(rets.len().saturating_sub(1))
                            .map(|t| self.zero(t))
                            .collect::<Result<_, _>>()?;
                        out.push(Value::Err(e));
                        let rv = if out.len() == 1 {
                            out.into_iter().next().unwrap()
                        } else {
                            Value::Tuple(out)
                        };
                        Ok(Ev::Ret(rv))
                    }
                    _ => Ok(Ev::V(match parts.len() {
                        0 => Value::Unit,
                        1 => parts.into_iter().next().unwrap(),
                        _ => Value::Tuple(parts),
                    })),
                }
            }
            K::Conv { target, arg } => {
                let v = match self.eval(arg)? {
                    Ev::V(v) => v,
                    r @ Ev::Ret(_) => return Ok(r),
                    Ev::PyErr(e) => {
                        return Ok(Ev::V(Value::Tuple(vec![self.zero(target)?, Value::Err(e)])))
                    }
                };
                self.convert(target, v).map(Ev::V)
            }
        }
    }

    fn binop(&mut self, op: BinOp, l: Value, r: Value) -> Result<Value, Fault> {
        use BinOp::*;
        use Value::*;
        let overflow = |x: Option<i64>| match x {
            Some(n) => Ok(Int(n)),
            None => Result::Err(self.fault("integer overflow")),
        };
        // list concat consumes both operands; the match below only borrows
        let (l, r) = match (op, l, r) {
            (Add, List(a), List(b)) => {
                // concat builds a fresh list; neither operand is mutated
                let mut out = a.borrow().clone();
                out.extend(b.borrow().iter().cloned());
                return Ok(Value::list(out));
            }
            (_, l, r) => (l, r),
        };
        let v = match (op, &l, &r) {
            (Add, Int(a), Int(b)) => overflow(a.checked_add(*b))?,
            (Sub, Int(a), Int(b)) => overflow(a.checked_sub(*b))?,
            (Mul, Int(a), Int(b)) => overflow(a.checked_mul(*b))?,
            (Div, Int(a), Int(b)) => {
                if *b == 0 {
                    return Result::Err(self.fault("division by zero"));
                }
                overflow(a.checked_div(*b))?
            }
            (Rem, Int(a), Int(b)) => {
                if *b == 0 {
                    return Result::Err(self.fault("division by zero"));
                }
                overflow(a.checked_rem(*b))?
            }
            (Add, Float(a), Float(b)) => Float(a + b),
            (Sub, Float(a), Float(b)) => Float(a - b),
            (Mul, Float(a), Float(b)) => Float(a * b),
            (Div, Float(a), Float(b)) => Float(a / b),
            (Add, Str(a), Str(b)) => Str(format!("{a}{b}")),
            (Eq, a, b) => match a.eq_value(b, 0) {
                Some(eq) => Bool(eq),
                None => return Result::Err(self.fault("value too deep or cyclic")),
            },
            (NotEq, a, b) => match a.eq_value(b, 0) {
                Some(eq) => Bool(!eq),
                None => return Result::Err(self.fault("value too deep or cyclic")),
            },
            (Lt, Int(a), Int(b)) => Bool(a < b),
            (LtEq, Int(a), Int(b)) => Bool(a <= b),
            (Gt, Int(a), Int(b)) => Bool(a > b),
            (GtEq, Int(a), Int(b)) => Bool(a >= b),
            (Lt, Float(a), Float(b)) => Bool(a < b),
            (LtEq, Float(a), Float(b)) => Bool(a <= b),
            (Gt, Float(a), Float(b)) => Bool(a > b),
            (GtEq, Float(a), Float(b)) => Bool(a >= b),
            (Lt, Str(a), Str(b)) => Bool(a < b),
            (LtEq, Str(a), Str(b)) => Bool(a <= b),
            (Gt, Str(a), Str(b)) => Bool(a > b),
            (GtEq, Str(a), Str(b)) => Bool(a >= b),
            _ => return Result::Err(self.fault("bad operands")),
        };
        Ok(v)
    }

    fn field(&self, r: &Value, name: &str) -> Result<Value, Fault> {
        match r {
            Value::Struct {
                fields,
                name: sname,
            } => fields
                .get(name)
                .cloned()
                .ok_or_else(|| self.fault(format!("{sname} has no field {name}"))),
            Value::Err(e) => Ok(match name {
                "msg" => Value::Str(e.msg.clone()),
                "origin" => Value::Str(e.origin.clone()),
                "pytype" => Value::Str(e.pytype.clone()),
                "traceback" => Value::Str(e.traceback.clone()),
                "cause" => match &e.cause {
                    Some(c) => Value::Err((**c).clone()),
                    None => Value::NoneV,
                },
                _ => return Err(self.fault(format!("error has no field {name}"))),
            }),
            Value::Module(m) => self.module_const(m, name),
            _ => Err(self.fault(format!("no field {name}"))),
        }
    }

    fn index(&self, r: &Value, i: Value) -> Result<Value, Fault> {
        match (r, i) {
            (Value::List(items), Value::Int(i)) => {
                let items = items.borrow();
                let len = items.len() as i64;
                if i < 0 || i >= len {
                    return Err(self.fault(format!("index out of bounds: {i} of {len}")));
                }
                Ok(items[i as usize].clone())
            }
            (Value::Str(s), Value::Int(i)) => {
                if i < 0 {
                    return Err(
                        self.fault(format!("index out of bounds: {i} of {}", s.chars().count()))
                    );
                }
                match s.chars().nth(i as usize) {
                    Some(c) => Ok(Value::Str(c.to_string())),
                    None => {
                        Err(self
                            .fault(format!("index out of bounds: {i} of {}", s.chars().count())))
                    }
                }
            }
            (Value::Map(m), k) => {
                let Some(key) = MapKey::from_value(&k) else {
                    return Err(self.fault("bad map key"));
                };
                Ok(m.borrow().get(&key).cloned().unwrap_or(Value::NoneV))
            }
            _ => Err(self.fault("cannot index this value")),
        }
    }

    fn slice(&self, r: &Value, lo: Value, hi: Value) -> Result<Value, Fault> {
        let (Value::Int(a), Value::Int(b)) = (lo, hi) else {
            return Err(self.fault("slice bounds must be int"));
        };
        match r {
            Value::List(items) => {
                let items = items.borrow();
                let len = items.len() as i64;
                if a < 0 || b < a || b > len {
                    return Err(self.fault(format!("slice out of bounds: {a}:{b} of {len}")));
                }
                Ok(Value::list(items[a as usize..b as usize].to_vec()))
            }
            Value::Str(s) => {
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                if a < 0 || b < a || b > len {
                    return Err(self.fault(format!("slice out of bounds: {a}:{b} of {len}")));
                }
                Ok(Value::Str(chars[a as usize..b as usize].iter().collect()))
            }
            _ => Err(self.fault("cannot slice this value")),
        }
    }

    fn error_builtin(&mut self, name: &str, mut args: Vec<Value>) -> Result<Value, Fault> {
        match name {
            "new" => match args.pop() {
                Some(Value::Str(msg)) => Ok(Value::Err(ErrVal {
                    msg,
                    origin: self.origin(),
                    ..Default::default()
                })),
                _ => Err(self.fault("error.new needs a str")),
            },
            "wrap" => {
                let (Some(Value::Str(msg)), Some(Value::Err(cause))) = (args.pop(), args.pop())
                else {
                    return Err(self.fault("error.wrap needs an error and a str"));
                };
                Ok(Value::Err(ErrVal {
                    msg,
                    origin: self.origin(),
                    cause: Some(Box::new(cause)),
                    ..Default::default()
                }))
            }
            _ => Err(self.fault(format!("error has no member {name}"))),
        }
    }

    /// Zero value for a return type, used when `check` fails out early.
    /// An unknown named type here means the checker let something through;
    /// that is an interpreter bug reported loudly, never a silent Unit.
    pub(crate) fn zero(&self, t: &TypeExpr) -> Result<Value, Fault> {
        Ok(match t {
            TypeExpr::Named(n) => match n.as_str() {
                "int" => Value::Int(0),
                "float" => Value::Float(0.0),
                "bool" => Value::Bool(false),
                "str" => Value::Str(String::new()),
                "error" => Value::NoneV,
                "py" => Value::Py(crate::bridge::py_none()),
                s => match self.structs.get(s) {
                    Some(fields) => {
                        let mut out = IndexMap::new();
                        for (f, ft) in fields {
                            out.insert(f.clone(), self.zero(ft)?);
                        }
                        Value::Struct {
                            name: s.to_string(),
                            fields: out,
                        }
                    }
                    None => {
                        return Err(
                            self.fault(format!("internal: no zero value for unknown type {s}"))
                        )
                    }
                },
            },
            TypeExpr::List(_) => Value::list(vec![]),
            TypeExpr::Map(..) => Value::map(IndexMap::new()),
            TypeExpr::Opt(_) => Value::NoneV,
            TypeExpr::Fn(..) => Value::Fn(FnRef::Zero),
        })
    }
}

pub fn truthy(v: &Value) -> bool {
    matches!(v, Value::Bool(true))
}

/// The free variables of a function literal: names its body reads that are
/// neither parameters nor declared within. These are what it captures.
fn free_vars(params: &[Param], body: &Block) -> HashSet<String> {
    struct Fv {
        free: HashSet<String>,
        bound: Vec<HashSet<String>>,
    }
    impl Fv {
        fn ident(&mut self, n: &str) {
            if n != "_" && !self.bound.iter().any(|s| s.contains(n)) {
                self.free.insert(n.to_string());
            }
        }
        fn declare(&mut self, n: &str) {
            if let Some(s) = self.bound.last_mut() {
                s.insert(n.to_string());
            }
        }
        fn block(&mut self, b: &Block) {
            self.bound.push(HashSet::new());
            for st in b {
                self.stmt(st);
            }
            self.bound.pop();
        }
        fn stmt(&mut self, s: &Stmt) {
            match &s.kind {
                StmtKind::Let { names, expr } => {
                    self.expr(expr);
                    for n in names {
                        self.declare(n);
                    }
                }
                StmtKind::Assign { target, expr } => {
                    self.expr(target);
                    self.expr(expr);
                }
                StmtKind::Expr(e) => self.expr(e),
                StmtKind::Return(es) => {
                    for e in es {
                        self.expr(e);
                    }
                }
                StmtKind::If {
                    cond,
                    then,
                    elifs,
                    els,
                } => {
                    self.expr(cond);
                    self.block(then);
                    for (c, b) in elifs {
                        self.expr(c);
                        self.block(b);
                    }
                    if let Some(b) = els {
                        self.block(b);
                    }
                }
                StmtKind::ForRange { names, iter, body } => {
                    self.expr(iter);
                    self.bound.push(names.iter().cloned().collect());
                    self.block(body);
                    self.bound.pop();
                }
                StmtKind::ForCond { cond, body } => {
                    if let Some(c) = cond {
                        self.expr(c);
                    }
                    self.block(body);
                }
                StmtKind::With { expr, body } => {
                    self.expr(expr);
                    self.block(body);
                }
                StmtKind::Break | StmtKind::Continue => {}
            }
        }
        fn expr(&mut self, e: &Expr) {
            match &e.kind {
                ExprKind::Ident(n) => self.ident(n),
                ExprKind::Int(_)
                | ExprKind::Float(_)
                | ExprKind::Str(_)
                | ExprKind::Bool(_)
                | ExprKind::NoneLit => {}
                ExprKind::List(items) | ExprKind::ListLit { items, .. } => {
                    for it in items {
                        self.expr(it);
                    }
                }
                ExprKind::MapLit { entries, .. } => {
                    for (k, v) in entries {
                        self.expr(k);
                        self.expr(v);
                    }
                }
                ExprKind::StructLit { fields, .. } => {
                    for (_, v) in fields {
                        self.expr(v);
                    }
                }
                ExprKind::Unary { rhs, .. } => self.expr(rhs),
                ExprKind::Binary { lhs, rhs, .. } => {
                    self.expr(lhs);
                    self.expr(rhs);
                }
                ExprKind::Call {
                    callee,
                    args,
                    kwargs,
                } => {
                    self.expr(callee);
                    for a in args {
                        self.expr(a);
                    }
                    for (_, v) in kwargs {
                        self.expr(v);
                    }
                }
                ExprKind::Method {
                    recv, args, kwargs, ..
                } => {
                    self.expr(recv);
                    for a in args {
                        self.expr(a);
                    }
                    for (_, v) in kwargs {
                        self.expr(v);
                    }
                }
                ExprKind::Field { recv, .. } => self.expr(recv),
                ExprKind::Index { recv, idx } => {
                    self.expr(recv);
                    self.expr(idx);
                }
                ExprKind::Slice { recv, lo, hi } => {
                    self.expr(recv);
                    self.expr(lo);
                    self.expr(hi);
                }
                ExprKind::Lambda { params, body, .. } => {
                    self.bound
                        .push(params.iter().map(|p| p.name.clone()).collect());
                    self.block(body);
                    self.bound.pop();
                }
                ExprKind::Check(inner) => self.expr(inner),
                ExprKind::Conv { arg, .. } => self.expr(arg),
            }
        }
    }
    let mut fv = Fv {
        free: HashSet::new(),
        bound: vec![params.iter().map(|p| p.name.clone()).collect()],
    };
    fv.block(body);
    fv.free
}
