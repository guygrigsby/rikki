use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::ast::*;
use crate::value::{ClosureData, ErrVal, FnRef, MapKey, Value};

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

/// val! for callers returning Result<Flow, Fault>.
macro_rules! sval {
    ($e:expr) => {
        match $e? {
            Ev::V(v) => v,
            Ev::Ret(r) => return Ok(Flow::Return(r)),
            // the checker forces py chains into check or destructure
            Ev::PyErr(e) => {
                return Err(Fault {
                    msg: format!("unhandled python error: {}", e.msg),
                    stack: vec![],
                })
            }
        }
    };
}

pub struct Interp<'p> {
    fns: HashMap<String, &'p FnDecl>,
    pub(crate) structs: HashMap<String, Vec<(String, TypeExpr)>>,
    globals: HashMap<String, Value>,
    py_imports: Vec<String>,
    scopes: Vec<HashMap<String, Value>>,
    saved: Vec<Vec<HashMap<String, Value>>>,
    /// Return types of the active function, for zero-filling check returns.
    ret_stack: Vec<Vec<TypeExpr>>,
    call_stack: Vec<String>,
    pub out: String,
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
                            let s = |n: &str| TypeExpr::Named(n.into());
                            let headers = TypeExpr::Map(
                                Box::new(s("str")),
                                Box::new(s("str")),
                            );
                            structs.insert(
                                "Request".into(),
                                vec![
                                    ("method".into(), s("str")),
                                    ("url".into(), s("str")),
                                    ("body".into(), s("str")),
                                    ("headers".into(), headers.clone()),
                                ],
                            );
                            structs.insert(
                                "Response".into(),
                                vec![
                                    ("status".into(), s("int")),
                                    ("body".into(), s("str")),
                                    ("headers".into(), headers),
                                ],
                            );
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
            out: String::new(),
        }
    }

    pub(crate) fn fault(&self, msg: impl Into<String>) -> Fault {
        Fault { msg: msg.into(), stack: self.call_stack.clone() }
    }

    /// Run fn main. Returns main's error value if it returned one.
    pub fn run_main(&mut self) -> Result<Option<ErrVal>, Fault> {
        for m in self.py_imports.clone() {
            match crate::bridge::import(&m) {
                Ok(h) => {
                    self.globals.insert(m, Value::Py(h));
                }
                Err(e) => {
                    return Err(self.fault(format!("import py \"{m}\": {}", e.msg)))
                }
            }
        }
        let v = self.call_named("main", vec![])?;
        Ok(match v {
            Value::Err(e) => Some(e),
            _ => None,
        })
    }

    /// Mongoose call-stack depth cap: past this the call faults instead of
    /// overflowing the Rust stack (each mongoose frame is several Rust
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
            scope.insert(p.name.clone(), a);
        }
        self.enter(name.to_string(), vec![scope], f.ret.clone());
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
            scope.insert(p.name.clone(), a);
        }
        self.enter("fn".into(), vec![c.captured.clone(), scope], c.ret.clone());
        // expression body: a lone expression statement yields its value
        let flow = if c.body.len() == 1 {
            if let StmtKind::Expr(e) = &c.body[0].kind {
                match self.eval(e) {
                    Ok(Ev::V(v)) | Ok(Ev::Ret(v)) => Ok(Flow::Return(v)),
                    Ok(Ev::PyErr(e)) => Err(Fault {
                        msg: format!("unhandled python error: {}", e.msg),
                        stack: vec![],
                    }),
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

    fn enter(&mut self, name: String, scopes: Vec<HashMap<String, Value>>, ret: Vec<TypeExpr>) {
        self.call_stack.push(name);
        self.ret_stack.push(ret);
        self.saved.push(std::mem::replace(&mut self.scopes, scopes));
    }

    fn leave(&mut self) {
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
                        Err(e) => {
                            return Err(self.fault(format!("import py {path:?}: {}", e.msg)))
                        }
                    }
                } else {
                    self.globals.insert(path.clone(), Value::Module(path.clone()));
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
        std::mem::take(&mut self.out)
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
        match &s.kind {
            StmtKind::Let { names, expr } => {
                let v = match self.eval(expr)? {
                    Ev::V(v) => v,
                    Ev::Ret(r) => return Ok(Flow::Return(r)),
                    Ev::PyErr(e) => {
                        // py chain destructured: zero-fill values, bind error
                        let mut parts = vec![Value::NoneV; names.len().saturating_sub(1)];
                        parts.push(Value::Err(e));
                        for (n, p) in names.iter().zip(parts) {
                            if n != "_" {
                                self.scopes.last_mut().unwrap().insert(n.clone(), p);
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
                        self.scopes.last_mut().unwrap().insert(n.clone(), p);
                    }
                }
                Ok(Flow::Normal)
            }
            StmtKind::Assign { target, expr } => {
                let v = sval!(self.eval(expr));
                match self.assign(target, v)? {
                    Ev::Ret(r) => Ok(Flow::Return(r)),
                    Ev::PyErr(e) => Err(Fault {
                        msg: format!("unhandled python error: {}", e.msg),
                        stack: vec![],
                    }),
                    Ev::V(_) => Ok(Flow::Normal),
                }
            }
            StmtKind::Expr(e) => {
                sval!(self.eval(e));
                Ok(Flow::Normal)
            }
            StmtKind::Return(exprs) => {
                let mut vals = vec![];
                for e in exprs {
                    vals.push(sval!(self.eval(e)));
                }
                let v = match vals.len() {
                    0 => Value::Unit,
                    1 => vals.into_iter().next().unwrap(),
                    _ => Value::Tuple(vals),
                };
                Ok(Flow::Return(v))
            }
            StmtKind::If { cond, then, elifs, els } => {
                if truthy(&sval!(self.eval(cond))) {
                    return self.exec_block(then);
                }
                for (c, b) in elifs {
                    if truthy(&sval!(self.eval(c))) {
                        return self.exec_block(b);
                    }
                }
                if let Some(b) = els {
                    return self.exec_block(b);
                }
                Ok(Flow::Normal)
            }
            StmtKind::ForIn { names, iter, body } => {
                let it = sval!(self.eval(iter));
                let rounds: Vec<Vec<Value>> = match it {
                    Value::List(items) => items.into_iter().map(|v| vec![v]).collect(),
                    Value::Map(m) => {
                        m.into_iter().map(|(k, v)| vec![k.to_value(), v]).collect()
                    }
                    _ => return Err(self.fault("cannot iterate this value")),
                };
                for round in rounds {
                    self.scopes.push(HashMap::new());
                    for (n, v) in names.iter().zip(round) {
                        if n != "_" {
                            self.scopes.last_mut().unwrap().insert(n.clone(), v);
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
                        if !truthy(&sval!(self.eval(c))) {
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
            StmtKind::Break => Ok(Flow::Break),
            StmtKind::Continue => Ok(Flow::Continue),
        }
    }

    // ---------- assignment ----------

    fn assign(&mut self, target: &Expr, v: Value) -> Result<Ev, Fault> {
        // collect the path down to a base identifier
        enum Step {
            Idx(Value),
            Field(String),
        }
        let mut steps = vec![];
        let mut cur = target;
        loop {
            match &cur.kind {
                ExprKind::Ident(name) => {
                    let name = name.clone();
                    steps.reverse();
                    // locate the variable
                    let mut slot: Option<&mut Value> = None;
                    for s in self.scopes.iter_mut().rev() {
                        if let Some(x) = s.get_mut(&name) {
                            slot = Some(x);
                            break;
                        }
                    }
                    let Some(mut slot) = slot else {
                        return Err(Fault {
                            msg: format!("undefined: {name}"),
                            stack: self.call_stack.clone(),
                        });
                    };
                    // descend
                    let stack = self.call_stack.clone();
                    let flt = |msg: &str| Fault { msg: msg.into(), stack: stack.clone() };
                    let n = steps.len();
                    for (i, st) in steps.into_iter().enumerate() {
                        let last = i + 1 == n;
                        match st {
                            Step::Field(f) => match slot {
                                Value::Struct { fields, .. } => match fields.get_mut(&f) {
                                    Some(x) => slot = x,
                                    None => return Err(flt("unknown field")),
                                },
                                _ => return Err(flt("cannot assign field here")),
                            },
                            Step::Idx(idx) => match slot {
                                Value::List(items) => {
                                    let i = match idx {
                                        Value::Int(i) => i,
                                        _ => return Err(flt("index must be int")),
                                    };
                                    let len = items.len() as i64;
                                    if i < 0 || i >= len {
                                        return Err(flt(&format!(
                                            "index out of bounds: {i} of {len}"
                                        )));
                                    }
                                    slot = &mut items[i as usize];
                                }
                                Value::Map(m) => {
                                    let Some(k) = MapKey::from_value(&idx) else {
                                        return Err(flt("bad map key"));
                                    };
                                    if last {
                                        m.insert(k, v);
                                        return Ok(Ev::V(Value::Unit));
                                    }
                                    match m.get_mut(&k) {
                                        Some(x) => slot = x,
                                        None => return Err(flt("missing key")),
                                    }
                                }
                                Value::Str(_) => {
                                    return Err(flt("cannot assign into a string"))
                                }
                                _ => return Err(flt("cannot index this value")),
                            },
                        }
                    }
                    *slot = v;
                    return Ok(Ev::V(Value::Unit));
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
                for s in self.scopes.iter().rev() {
                    if let Some(v) = s.get(n) {
                        return ok(v.clone());
                    }
                }
                if self.fns.contains_key(n) {
                    return ok(Value::Fn(FnRef::Decl(n.clone())));
                }
                if let Some(v) = self.globals.get(n) {
                    return ok(v.clone());
                }
                Err(self.fault(format!("undefined: {n}")))
            }
            K::List(items) => {
                let mut out = vec![];
                for it in items {
                    out.push(val!(self.eval(it)));
                }
                ok(Value::List(out))
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
                ok(Value::Map(m))
            }
            K::StructLit { name, fields } => {
                let def = self
                    .structs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| self.fault(format!("unknown struct: {name}")))?;
                let mut vals = HashMap::new();
                for (f, v) in fields {
                    vals.insert(f.clone(), val!(self.eval(v)));
                }
                // field order follows the declaration
                let mut out = IndexMap::new();
                for (f, _) in &def {
                    match vals.remove(f) {
                        Some(v) => {
                            out.insert(f.clone(), v);
                        }
                        None => return Err(self.fault(format!("missing field: {f}"))),
                    }
                }
                ok(Value::Struct { name: name.clone(), fields: out })
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
                        Err(e) => Ev::PyErr(e),
                    });
                }
                self.binop(*op, l, r).map(Ev::V)
            }
            K::Call { callee, args } => {
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
                    return Ok(match crate::bridge::call(h, &vals) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(e),
                    });
                }
                self.call_value(&f, vals).map(Ev::V)
            }
            K::Method { recv, name, args } => {
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
                        Err(e) => return Ok(Ev::PyErr(e)),
                    };
                    let Value::Py(fh) = &f else { unreachable!() };
                    return Ok(match crate::bridge::call(fh, &vals) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(e),
                    });
                }
                self.method_call(r, name, vals).map(Ev::V)
            }
            K::Field { recv, name } => {
                let r = val!(self.eval(recv));
                if let Value::Py(h) = &r {
                    return Ok(match crate::bridge::getattr(h, name) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(e),
                    });
                }
                self.field(r, name).map(Ev::V)
            }
            K::Index { recv, idx } => {
                let r = val!(self.eval(recv));
                let i = val!(self.eval(idx));
                if let Value::Py(h) = &r {
                    return Ok(match crate::bridge::index(h, &i) {
                        Ok(v) => Ev::V(v),
                        Err(e) => Ev::PyErr(e),
                    });
                }
                self.index(r, i).map(Ev::V)
            }
            K::Slice { recv, lo, hi } => {
                let r = val!(self.eval(recv));
                let lo = val!(self.eval(lo));
                let hi = val!(self.eval(hi));
                self.slice(r, lo, hi).map(Ev::V)
            }
            K::Lambda { params, ret, body } => {
                // capture by value: flatten visible scopes, inner shadows outer
                let mut captured = HashMap::new();
                for s in &self.scopes {
                    for (k, v) in s {
                        captured.insert(k.clone(), v.clone());
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
                            .collect();
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
                        return Ok(Ev::V(Value::Tuple(vec![
                            self.zero(target),
                            Value::Err(e),
                        ])))
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
            (Add, List(a), List(b)) => {
                let mut out = a.clone();
                out.extend(b.iter().cloned());
                List(out)
            }
            (Eq, a, b) => Bool(a.eq_value(b)),
            (NotEq, a, b) => Bool(!a.eq_value(b)),
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

    fn field(&mut self, r: Value, name: &str) -> Result<Value, Fault> {
        match r {
            Value::Struct { fields, name: sname } => fields
                .get(name)
                .cloned()
                .ok_or_else(|| self.fault(format!("{sname} has no field {name}"))),
            Value::Err(e) => Ok(match name {
                "msg" => Value::Str(e.msg),
                "pytype" => Value::Str(e.pytype),
                "traceback" => Value::Str(e.traceback),
                "cause" => match e.cause {
                    Some(c) => Value::Err(*c),
                    None => Value::NoneV,
                },
                _ => return Err(self.fault(format!("error has no field {name}"))),
            }),
            Value::Module(m) => self.module_const(&m, name),
            _ => Err(self.fault(format!("no field {name}"))),
        }
    }

    fn index(&mut self, r: Value, i: Value) -> Result<Value, Fault> {
        match (r, i) {
            (Value::List(items), Value::Int(i)) => {
                let len = items.len() as i64;
                if i < 0 || i >= len {
                    return Err(self.fault(format!("index out of bounds: {i} of {len}")));
                }
                Ok(items[i as usize].clone())
            }
            (Value::Str(s), Value::Int(i)) => {
                let len = s.chars().count() as i64;
                if i < 0 || i >= len {
                    return Err(self.fault(format!("index out of bounds: {i} of {len}")));
                }
                Ok(Value::Str(s.chars().nth(i as usize).unwrap().to_string()))
            }
            (Value::Map(m), k) => {
                let Some(key) = MapKey::from_value(&k) else {
                    return Err(self.fault("bad map key"));
                };
                Ok(m.get(&key).cloned().unwrap_or(Value::NoneV))
            }
            _ => Err(self.fault("cannot index this value")),
        }
    }

    fn slice(&mut self, r: Value, lo: Value, hi: Value) -> Result<Value, Fault> {
        let (Value::Int(a), Value::Int(b)) = (lo, hi) else {
            return Err(self.fault("slice bounds must be int"));
        };
        match r {
            Value::List(items) => {
                let len = items.len() as i64;
                if a < 0 || b < a || b > len {
                    return Err(self.fault(format!("slice out of bounds: {a}:{b} of {len}")));
                }
                Ok(Value::List(items[a as usize..b as usize].to_vec()))
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
                Some(Value::Str(msg)) => {
                    Ok(Value::Err(ErrVal { msg, ..Default::default() }))
                }
                _ => Err(self.fault("error.new needs a str")),
            },
            "wrap" => {
                let (Some(Value::Str(msg)), Some(Value::Err(cause))) = (args.pop(), args.pop())
                else {
                    return Err(self.fault("error.wrap needs an error and a str"));
                };
                Ok(Value::Err(ErrVal {
                    msg,
                    cause: Some(Box::new(cause)),
                    ..Default::default()
                }))
            }
            _ => Err(self.fault(format!("error has no member {name}"))),
        }
    }

    /// Zero value for a return type, used when `check` fails out early.
    pub(crate) fn zero(&self, t: &TypeExpr) -> Value {
        match t {
            TypeExpr::Named(n) => match n.as_str() {
                "int" => Value::Int(0),
                "float" => Value::Float(0.0),
                "bool" => Value::Bool(false),
                "str" => Value::Str(String::new()),
                "error" => Value::NoneV,
                "py" => Value::NoneV, // placeholder until the bridge lands
                s => match self.structs.get(s) {
                    Some(fields) => {
                        let mut out = IndexMap::new();
                        for (f, ft) in fields {
                            out.insert(f.clone(), self.zero(ft));
                        }
                        Value::Struct { name: s.to_string(), fields: out }
                    }
                    None => Value::Unit,
                },
            },
            TypeExpr::List(_) => Value::List(vec![]),
            TypeExpr::Map(..) => Value::Map(IndexMap::new()),
            TypeExpr::Opt(_) => Value::NoneV,
            TypeExpr::Fn(..) => Value::Fn(FnRef::Zero),
        }
    }
}

pub fn truthy(v: &Value) -> bool {
    matches!(v, Value::Bool(true))
}
