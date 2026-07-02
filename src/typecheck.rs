use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::Diag;
use crate::types::Type;

/// Result of checking an expression: one value or a multi-value (call/conv/check).
#[derive(Debug, Clone, PartialEq)]
pub enum ExprTy {
    One(Type),
    Multi(Vec<Type>),
    /// A chain of py operations: acts as `py` inside further postfix ops and
    /// as `(py, error?)` at the point of consumption.
    PyChain,
}

pub fn check(prog: &Program) -> Result<(), Vec<Diag>> {
    let mut c = Checker::default();
    c.collect(prog);
    c.check_program(prog);
    if c.diags.is_empty() {
        Ok(())
    } else {
        Err(c.diags)
    }
}

#[derive(Default)]
struct Scope {
    vars: HashMap<String, Type>,
    /// Flow-narrowed types, masking `vars` in this scope and outer ones.
    refits: HashMap<String, Type>,
}

#[derive(Default)]
struct Checker {
    structs: HashMap<String, Vec<(String, Type)>>,
    fns: HashMap<String, (Vec<Type>, Vec<Type>)>,
    imports: HashMap<String, ImportKind>,
    scopes: Vec<Scope>,
    current_ret: Vec<Type>,
    loop_depth: u32,
    diags: Vec<Diag>,
}

#[derive(Clone, PartialEq)]
enum ImportKind {
    Py,
    Std(String),
    File(String),
}

const STD_MODULES: &[&str] = &["math", "error", "file", "ctx", "http"];

enum Member {
    Fn(Vec<Type>, Vec<Type>),
    Const(Type),
}

fn err_opt() -> Type {
    Type::Opt(Box::new(Type::Error))
}

fn std_member(module: &str, name: &str) -> Option<Member> {
    use Type::*;
    let ctx = || Struct("Ctx".into());
    let resp = || Struct("Response".into());
    let m = match (module, name) {
        ("math", "sqrt") => Member::Fn(vec![Float], vec![Float]),
        ("math", "pow") => Member::Fn(vec![Float, Float], vec![Float]),
        ("math", "floor") | ("math", "ceil") | ("math", "round") => {
            Member::Fn(vec![Float], vec![Int])
        }
        ("math", "pi") | ("math", "e") => Member::Const(Float),
        // abs/min/max are polymorphic; handled in method checking directly
        ("error", "new") => Member::Fn(vec![Str], vec![Error]),
        ("error", "wrap") => Member::Fn(vec![Error, Str], vec![Error]),
        ("file", "read") => Member::Fn(vec![Str], vec![Str, err_opt()]),
        ("file", "write") | ("file", "append") => Member::Fn(vec![Str, Str], vec![err_opt()]),
        ("file", "exists") => Member::Fn(vec![Str], vec![Bool]),
        ("file", "list") => Member::Fn(vec![Str], vec![List(Box::new(Str)), err_opt()]),
        ("file", "remove") | ("file", "mkdir") => Member::Fn(vec![Str], vec![err_opt()]),
        ("ctx", "background") => Member::Fn(vec![], vec![ctx()]),
        ("ctx", "timeout") => Member::Fn(vec![ctx(), Float], vec![ctx()]),
        ("ctx", "interrupt") => Member::Fn(vec![ctx()], vec![ctx()]),
        ("http", "get") => Member::Fn(vec![ctx(), Str], vec![resp(), err_opt()]),
        ("http", "post") => Member::Fn(vec![ctx(), Str, Str], vec![resp(), err_opt()]),
        ("http", "request") => {
            Member::Fn(vec![ctx(), Struct("Request".into())], vec![resp(), err_opt()])
        }
        _ => return None,
    };
    Some(m)
}

impl Checker {
    fn diag(&mut self, line: u32, col: u32, msg: impl Into<String>) {
        self.diags.push(Diag { msg: msg.into(), line, col });
    }

    // ---------- collection pass ----------

    fn collect(&mut self, prog: &Program) {
        for d in &prog.decls {
            if let Decl::Import { path, py, line: _, col: _ } = d {
                if *py {
                    self.imports.insert(path.clone(), ImportKind::Py);
                } else if STD_MODULES.contains(&path.as_str()) {
                    self.imports.insert(path.clone(), ImportKind::Std(path.clone()));
                    if path == "http" {
                        self.structs.insert(
                            "Request".into(),
                            vec![
                                ("method".into(), Type::Str),
                                ("url".into(), Type::Str),
                                ("body".into(), Type::Str),
                                (
                                    "headers".into(),
                                    Type::Map(Box::new(Type::Str), Box::new(Type::Str)),
                                ),
                            ],
                        );
                        self.structs.insert(
                            "Response".into(),
                            vec![
                                ("status".into(), Type::Int),
                                ("body".into(), Type::Str),
                                (
                                    "headers".into(),
                                    Type::Map(Box::new(Type::Str), Box::new(Type::Str)),
                                ),
                            ],
                        );
                    }
                    if path == "ctx" {
                        self.structs.insert("Ctx".into(), vec![]);
                    }
                }
                // file imports resolve after fns and structs are collected
            }
        }
        for d in &prog.decls {
            if let Decl::Struct { name, line, col, .. } = d {
                if self.structs.contains_key(name) {
                    self.diag(*line, *col, format!("duplicate struct: {name}"));
                    continue;
                }
                self.structs.insert(name.clone(), vec![]);
            }
        }
        for d in &prog.decls {
            if let Decl::Struct { name, fields, line, col } = d {
                let fs: Vec<(String, Type)> = fields
                    .iter()
                    .map(|(f, t)| (f.clone(), self.resolve(t, *line, *col)))
                    .collect();
                self.structs.insert(name.clone(), fs);
            }
        }
        // a by-value field cycle can never be constructed; an option, list,
        // or map along the way breaks the cycle
        for d in &prog.decls {
            if let Decl::Struct { name, line, col, .. } = d {
                let cycle = self.structs.get(name).into_iter().flatten().find_map(|(f, t)| {
                    match t {
                        Type::Struct(s) if s == name || self.reaches_by_value(s, name) => {
                            Some((f.clone(), s.clone()))
                        }
                        _ => None,
                    }
                });
                if let Some((field, fty)) = cycle {
                    self.diag(
                        *line,
                        *col,
                        format!("recursive struct {name}: use an option ({field}: {fty}?)"),
                    );
                }
            }
        }
        for d in &prog.decls {
            if let Decl::Fn(f) = d {
                let mut params = vec![];
                for p in &f.params {
                    match &p.ty {
                        Some(t) => params.push(self.resolve(t, f.line, f.col)),
                        None => {
                            self.diag(f.line, f.col, format!("parameter {} needs a type", p.name));
                            params.push(Type::Unknown);
                        }
                    }
                }
                let rets = f.ret.iter().map(|t| self.resolve(t, f.line, f.col)).collect();
                if self.fns.insert(f.name.clone(), (params, rets)).is_some() {
                    self.diag(f.line, f.col, format!("duplicate function: {}", f.name));
                }
            }
        }
        // second import pass: a path that isn't stdlib or py is a file import
        // if the merged program has symbols under its namespace
        for d in &prog.decls {
            if let Decl::Import { path, py: false, line, col } = d {
                if STD_MODULES.contains(&path.as_str()) {
                    continue;
                }
                let prefix = format!("{path}.");
                let has_symbols = self.fns.keys().any(|k| k.starts_with(&prefix))
                    || self.structs.keys().any(|k| k.starts_with(&prefix));
                if has_symbols {
                    self.imports.insert(path.clone(), ImportKind::File(path.clone()));
                } else {
                    self.diag(*line, *col, format!("unknown module: {path}"));
                }
            }
        }
    }

    /// Whether struct `from` holds a `target` by value, transitively through
    /// struct-typed fields only.
    fn reaches_by_value(&self, from: &str, target: &str) -> bool {
        let mut seen = HashSet::new();
        let mut stack = vec![from.to_string()];
        while let Some(s) = stack.pop() {
            if !seen.insert(s.clone()) {
                continue;
            }
            for (_, t) in self.structs.get(&s).into_iter().flatten() {
                if let Type::Struct(next) = t {
                    if next == target {
                        return true;
                    }
                    stack.push(next.clone());
                }
            }
        }
        false
    }

    fn resolve(&mut self, t: &TypeExpr, line: u32, col: u32) -> Type {
        match t {
            TypeExpr::Named(n) => match n.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "bool" => Type::Bool,
                "str" => Type::Str,
                "error" => Type::Error,
                "py" => Type::Py,
                other => {
                    if self.structs.contains_key(other) {
                        Type::Struct(other.to_string())
                    } else {
                        self.diag(line, col, format!("unknown type: {other}"));
                        Type::Unknown
                    }
                }
            },
            TypeExpr::List(inner) => Type::List(Box::new(self.resolve(inner, line, col))),
            TypeExpr::Map(k, v) => {
                let kt = self.resolve(k, line, col);
                if !matches!(kt, Type::Int | Type::Str | Type::Bool | Type::Unknown) {
                    self.diag(line, col, format!("map key type must be int, str, or bool, got {kt}"));
                }
                Type::Map(Box::new(kt), Box::new(self.resolve(v, line, col)))
            }
            TypeExpr::Opt(inner) => Type::Opt(Box::new(self.resolve(inner, line, col))),
            TypeExpr::Fn(args, rets) => Type::Fn(
                args.iter().map(|a| self.resolve(a, line, col)).collect(),
                rets.iter().map(|r| self.resolve(r, line, col)).collect(),
            ),
        }
    }

    // ---------- scopes ----------

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.refits.get(name) {
                return Some(t.clone());
            }
            if let Some(t) = s.vars.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    fn declared(&self, name: &str) -> Option<Type> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.vars.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    fn declare(&mut self, name: &str, ty: Type, line: u32, col: u32) {
        if name == "_" {
            return;
        }
        if self.scopes.last().unwrap().vars.contains_key(name) {
            self.diag(line, col, format!("already declared: {name}"));
        }
        self.scopes.last_mut().unwrap().vars.insert(name.to_string(), ty);
    }

    fn refine(&mut self, name: &str, ty: Type) {
        self.scopes.last_mut().unwrap().refits.insert(name.to_string(), ty);
    }

    /// Drops every refinement of `name`, in all scopes. Assignments can
    /// happen in a nested scope, so the whole stack must forget; losing a
    /// refinement is always sound.
    fn invalidate(&mut self, name: &str) {
        for s in &mut self.scopes {
            s.refits.remove(name);
        }
    }

    // ---------- program ----------

    fn check_program(&mut self, prog: &Program) {
        match self.fns.get("main") {
            None => self.diag(1, 1, "missing fn main"),
            Some((params, rets)) => {
                let ok = params.is_empty() && (rets.is_empty() || rets == &[err_opt()]);
                if !ok {
                    self.diag(1, 1, "fn main takes no parameters and returns nothing or (error?)");
                }
            }
        }
        for d in &prog.decls {
            if let Decl::Fn(f) = d {
                self.check_fn(f);
            }
        }
    }

    fn check_fn(&mut self, f: &FnDecl) {
        self.current_ret = self.fns.get(&f.name).map(|(_, r)| r.clone()).unwrap_or_default();
        self.push_scope();
        let param_tys: Vec<Type> =
            self.fns.get(&f.name).map(|(p, _)| p.clone()).unwrap_or_default();
        for (p, t) in f.params.iter().zip(param_tys) {
            self.declare(&p.name, t, f.line, f.col);
        }
        let diverges = self.check_block(&f.body);
        self.pop_scope();
        if !self.current_ret.is_empty() && !diverges {
            self.diag(f.line, f.col, format!("missing return in {}", f.name));
        }
    }

    /// Checks a block in a fresh scope; returns whether it always diverges.
    fn check_block(&mut self, b: &Block) -> bool {
        self.push_scope();
        let mut diverges = false;
        for s in b {
            if diverges {
                self.diag(s.line, s.col, "unreachable code");
                break;
            }
            diverges = self.check_stmt(s);
        }
        self.pop_scope();
        diverges
    }

    // ---------- statements ----------

    fn check_stmt(&mut self, s: &Stmt) -> bool {
        let (line, col) = (s.line, s.col);
        match &s.kind {
            StmtKind::Let { names, expr } => {
                let ty = self.check_expr(expr, None);
                let supplied = match ty {
                    ExprTy::One(t) => vec![t],
                    ExprTy::Multi(ts) => ts,
                    ExprTy::PyChain => vec![Type::Py, err_opt()],
                };
                if names.len() == supplied.len() {
                    for (n, t) in names.iter().zip(supplied) {
                        if t == Type::Unit {
                            self.diag(line, col, format!("{n} would have no value"));
                        }
                        self.declare(n, t, line, col);
                    }
                } else if names.len() == supplied.len() - 1 && supplied.last() == Some(&err_opt()) {
                    self.diag(line, col, "error result must be handled");
                } else {
                    self.diag(
                        line,
                        col,
                        format!("expected {} values, got {}", names.len(), supplied.len()),
                    );
                    for n in names {
                        self.declare(n, Type::Unknown, line, col);
                    }
                }
                false
            }
            StmtKind::Assign { target, expr } => {
                let target_ty = match &target.kind {
                    ExprKind::Ident(n) => match self.declared(n) {
                        Some(t) => {
                            // assignment invalidates any narrowing
                            self.invalidate(n);
                            t
                        }
                        None => {
                            self.diag(line, col, format!("undefined: {n}"));
                            Type::Unknown
                        }
                    },
                    ExprKind::Index { .. } | ExprKind::Field { .. } => {
                        match self.check_expr(target, None) {
                            ExprTy::PyChain => {
                                self.diag(line, col, "cannot assign into a py expression");
                                Type::Unknown
                            }
                            ExprTy::One(t) => match (&target.kind, t) {
                                // map read is V?, but assignment writes a V
                                (ExprKind::Index { recv, .. }, t) => {
                                    let recv_ty = self.expr_one(recv, None);
                                    match recv_ty {
                                        Type::Map(_, v) => *v,
                                        _ => t,
                                    }
                                }
                                (_, t) => t,
                            },
                            ExprTy::Multi(_) => {
                                self.diag(line, col, "cannot assign to multiple values");
                                Type::Unknown
                            }
                        }
                    }
                    _ => {
                        self.diag(line, col, "cannot assign to this expression");
                        Type::Unknown
                    }
                };
                let val = self.expr_one(expr, Some(&target_ty));
                if !target_ty.accepts(&val) {
                    self.diag(line, col, format!("expected {target_ty}, got {val}"));
                }
                false
            }
            StmtKind::Expr(e) => {
                let ty = self.check_expr(e, None);
                match ty {
                    ExprTy::PyChain => {
                        self.diag(line, col, "error result must be handled");
                    }
                    ExprTy::Multi(ts) if ts.last() == Some(&err_opt()) => {
                        self.diag(line, col, "error result must be handled");
                    }
                    ExprTy::One(t) if t == err_opt() && !matches!(e.kind, ExprKind::Check(_)) => {
                        self.diag(line, col, "error result must be handled");
                    }
                    _ => {}
                }
                false
            }
            StmtKind::Return(exprs) => {
                let want = self.current_ret.clone();
                if exprs.is_empty() && !want.is_empty() {
                    // allow bare `return` only when everything has a zero... no: require values
                    self.diag(line, col, format!("expected {} return values", want.len()));
                } else if exprs.len() != want.len() {
                    self.diag(
                        line,
                        col,
                        format!("expected {} return values, got {}", want.len(), exprs.len()),
                    );
                } else {
                    for (e, w) in exprs.iter().zip(&want) {
                        let t = self.expr_one(e, Some(w));
                        if !w.accepts(&t) {
                            self.diag(e.line, e.col, format!("expected {w}, got {t}"));
                        }
                    }
                }
                true
            }
            StmtKind::If { cond, then, elifs, els } => {
                self.check_cond(cond);
                // then-branch with positive narrowing
                self.push_scope();
                for (n, t) in self.narrowing(cond, true) {
                    self.refine(&n, t);
                }
                let then_div = self.check_block_inline(then);
                self.pop_scope();

                let mut all_div = then_div;
                for (c, b) in elifs {
                    self.check_cond(c);
                    self.push_scope();
                    for (n, t) in self.narrowing(c, true) {
                        self.refine(&n, t);
                    }
                    all_div &= self.check_block_inline(b);
                    self.pop_scope();
                }
                let els_div = match els {
                    Some(b) => {
                        self.push_scope();
                        if elifs.is_empty() {
                            for (n, t) in self.narrowing(cond, false) {
                                self.refine(&n, t);
                            }
                        }
                        let d = self.check_block_inline(b);
                        self.pop_scope();
                        d
                    }
                    None => false,
                };
                // terminal narrowing: if one side always diverges, the code
                // after the if only runs on the other side's condition
                if elifs.is_empty() {
                    if then_div && els.is_none() {
                        for (n, t) in self.narrowing(cond, false) {
                            self.refine(&n, t);
                        }
                    }
                    if els_div && !then_div {
                        for (n, t) in self.narrowing(cond, true) {
                            self.refine(&n, t);
                        }
                    }
                }
                all_div && els_div
            }
            StmtKind::ForIn { names, iter, body } => {
                let it = self.expr_one(iter, None);
                self.push_scope();
                match (&it, names.len()) {
                    (Type::List(t), 1) => self.declare(&names[0], (**t).clone(), line, col),
                    (Type::Map(k, v), 2) => {
                        self.declare(&names[0], (**k).clone(), line, col);
                        self.declare(&names[1], (**v).clone(), line, col);
                    }
                    (Type::Unknown, _) => {
                        for n in names {
                            self.declare(n, Type::Unknown, line, col);
                        }
                    }
                    _ => {
                        self.diag(
                            line,
                            col,
                            format!("cannot iterate {it} with {} names", names.len()),
                        );
                        for n in names {
                            self.declare(n, Type::Unknown, line, col);
                        }
                    }
                }
                self.invalidate_loop_assigns(body);
                self.loop_depth += 1;
                self.check_block_inline(body);
                self.loop_depth -= 1;
                self.pop_scope();
                false
            }
            StmtKind::ForCond { cond, body } => {
                if let Some(c) = cond {
                    self.check_cond(c);
                }
                self.invalidate_loop_assigns(body);
                self.loop_depth += 1;
                let _ = self.check_block(body);
                self.loop_depth -= 1;
                // infinite loop without break diverges
                cond.is_none() && !contains_break(body)
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.diag(line, col, "break or continue outside loop");
                }
                true
            }
        }
    }

    /// A loop body runs more than once: an assignment anywhere in it kills
    /// narrowing for the whole body, not just the statements after it.
    fn invalidate_loop_assigns(&mut self, body: &Block) {
        let mut names = vec![];
        assigned_idents(body, &mut names);
        for n in &names {
            self.invalidate(n);
        }
    }

    /// Like check_block but without pushing a scope (caller already did, to
    /// seed refinements).
    fn check_block_inline(&mut self, b: &Block) -> bool {
        self.push_scope();
        let mut diverges = false;
        for s in b {
            if diverges {
                self.diag(s.line, s.col, "unreachable code");
                break;
            }
            diverges = self.check_stmt(s);
        }
        self.pop_scope();
        diverges
    }

    fn check_cond(&mut self, cond: &Expr) {
        let t = self.expr_one(cond, Some(&Type::Bool));
        if !matches!(t, Type::Bool | Type::Unknown) {
            self.diag(cond.line, cond.col, format!("condition must be bool, got {t}"));
        }
    }

    /// Narrowings implied by `cond` being `positive`.
    /// Only `x != none` / `x == none` forms narrow.
    fn narrowing(&mut self, cond: &Expr, positive: bool) -> Vec<(String, Type)> {
        if let ExprKind::Binary { op, lhs, rhs } = &cond.kind {
            let (ident, other) = match (&lhs.kind, &rhs.kind) {
                (ExprKind::Ident(n), _) => (Some(n), rhs),
                (_, ExprKind::Ident(n)) => (Some(n), lhs),
                _ => (None, lhs),
            };
            if let (Some(name), ExprKind::NoneLit) = (ident, &other.kind) {
                if let Some(Type::Opt(inner)) = self.lookup(name) {
                    let narrows = match op {
                        BinOp::NotEq => positive,
                        BinOp::Eq => !positive,
                        _ => return vec![],
                    };
                    if narrows {
                        return vec![(name.clone(), (*inner).clone())];
                    }
                }
            }
        }
        vec![]
    }

    // ---------- expressions ----------

    fn expr_one(&mut self, e: &Expr, expected: Option<&Type>) -> Type {
        match self.check_expr(e, expected) {
            ExprTy::One(t) => t,
            ExprTy::Multi(_) => {
                self.diag(e.line, e.col, "multiple values in single-value context");
                Type::Unknown
            }
            ExprTy::PyChain => {
                self.diag(e.line, e.col, "error result must be handled");
                Type::Py
            }
        }
    }

    /// Like expr_one but lets a py chain through as `py` (for contexts that
    /// absorb its fallibility: conversions, operators, further chain links).
    fn expr_pyish(&mut self, e: &Expr, expected: Option<&Type>) -> Type {
        match self.check_expr(e, expected) {
            ExprTy::One(t) => t,
            ExprTy::Multi(_) => {
                self.diag(e.line, e.col, "multiple values in single-value context");
                Type::Unknown
            }
            ExprTy::PyChain => Type::Py,
        }
    }

    fn check_expr(&mut self, e: &Expr, expected: Option<&Type>) -> ExprTy {
        use ExprKind as K;
        let one = ExprTy::One;
        let (line, col) = (e.line, e.col);
        match &e.kind {
            K::Int(_) => one(Type::Int),
            K::Float(_) => one(Type::Float),
            K::Str(_) => one(Type::Str),
            K::Bool(_) => one(Type::Bool),
            K::NoneLit => one(Type::Opt(Box::new(Type::Unknown))),
            K::Ident(n) => {
                if let Some(t) = self.lookup(n) {
                    return one(t);
                }
                if let Some((args, rets)) = self.fns.get(n) {
                    return one(Type::Fn(args.clone(), rets.clone()));
                }
                if let Some(k) = self.imports.get(n) {
                    return one(match k {
                        ImportKind::Py => Type::Py,
                        ImportKind::Std(m) | ImportKind::File(m) => Type::Module(m.clone()),
                    });
                }
                self.diag(line, col, format!("undefined: {n}"));
                one(Type::Unknown)
            }
            K::List(items) => {
                let expected_elem = match expected {
                    Some(Type::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                if items.is_empty() && expected_elem.is_none() {
                    self.diag(
                        line,
                        col,
                        "cannot infer element type of []; use it where a list type is expected",
                    );
                }
                let mut elem = expected_elem.unwrap_or(Type::Unknown);
                for it in items {
                    let t = self.expr_one(it, Some(&elem));
                    if elem == Type::Unknown {
                        elem = t;
                    } else if !elem.accepts(&t) {
                        self.diag(it.line, it.col, format!("expected {elem}, got {t}"));
                    }
                }
                one(Type::List(Box::new(elem)))
            }
            K::MapLit { key, val, entries } => {
                let kt = self.resolve(key, line, col);
                let vt = self.resolve(val, line, col);
                for (k, v) in entries {
                    let got_k = self.expr_one(k, Some(&kt));
                    if !kt.accepts(&got_k) {
                        self.diag(k.line, k.col, format!("expected {kt}, got {got_k}"));
                    }
                    let got_v = self.expr_one(v, Some(&vt));
                    if !vt.accepts(&got_v) {
                        self.diag(v.line, v.col, format!("expected {vt}, got {got_v}"));
                    }
                }
                one(Type::Map(Box::new(kt), Box::new(vt)))
            }
            K::StructLit { name, fields } => {
                // Ctx is opaque: the checker knows it as a struct, the
                // interpreter does not; only the ctx module makes one
                if name == "Ctx" && matches!(self.imports.get("ctx"), Some(ImportKind::Std(_))) {
                    self.diag(line, col, "Ctx cannot be constructed; use ctx.background()");
                    return one(Type::Struct(name.clone()));
                }
                let Some(def) = self.structs.get(name).cloned() else {
                    self.diag(line, col, format!("unknown struct: {name}"));
                    return one(Type::Unknown);
                };
                for (fname, fty) in &def {
                    match fields.iter().find(|(n, _)| n == fname) {
                        Some((_, v)) => {
                            let t = self.expr_one(v, Some(fty));
                            if !fty.accepts(&t) {
                                self.diag(v.line, v.col, format!("expected {fty}, got {t}"));
                            }
                        }
                        None => self.diag(line, col, format!("missing field: {fname}")),
                    }
                }
                for (fname, _) in fields {
                    if !def.iter().any(|(n, _)| n == fname) {
                        self.diag(line, col, format!("unknown field: {fname}"));
                    }
                }
                one(Type::Struct(name.clone()))
            }
            K::Unary { op, rhs } => {
                let t = self.expr_one(rhs, None);
                match op {
                    UnOp::Not => {
                        if !matches!(t, Type::Bool | Type::Unknown) {
                            self.diag(line, col, format!("! needs bool, got {t}"));
                        }
                        one(Type::Bool)
                    }
                    UnOp::Neg => {
                        if !matches!(t, Type::Int | Type::Float | Type::Unknown) {
                            self.diag(line, col, format!("- needs int or float, got {t}"));
                        }
                        one(t)
                    }
                }
            }
            K::Binary { op, lhs, rhs } => {
                let t = self.binary(*op, lhs, rhs, line, col);
                if t == Type::Py {
                    ExprTy::PyChain
                } else {
                    one(t)
                }
            }
            K::Call { callee, args } => self.call(callee, args, line, col),
            K::Method { recv, name, args } => self.method(recv, name, args, line, col),
            K::Field { recv, name } => {
                let t = self.field(recv, name, line, col);
                if t == Type::Py {
                    ExprTy::PyChain
                } else {
                    one(t)
                }
            }
            K::Index { recv, idx } => {
                let rt = self.expr_pyish(recv, None);
                if rt == Type::Py {
                    self.expr_one(idx, None);
                    return ExprTy::PyChain;
                }
                match rt {
                    Type::List(t) => {
                        let it = self.expr_one(idx, Some(&Type::Int));
                        if !matches!(it, Type::Int | Type::Unknown) {
                            self.diag(idx.line, idx.col, format!("index must be int, got {it}"));
                        }
                        one(*t)
                    }
                    Type::Map(k, v) => {
                        let it = self.expr_one(idx, Some(&k));
                        if !k.accepts(&it) {
                            self.diag(idx.line, idx.col, format!("expected {k}, got {it}"));
                        }
                        one(Type::Opt(v))
                    }
                    Type::Str => {
                        let it = self.expr_one(idx, Some(&Type::Int));
                        if !matches!(it, Type::Int | Type::Unknown) {
                            self.diag(idx.line, idx.col, format!("index must be int, got {it}"));
                        }
                        one(Type::Str)
                    }
                    Type::Unknown => one(Type::Unknown),
                    t => {
                        self.diag(line, col, format!("cannot index {t}"));
                        one(Type::Unknown)
                    }
                }
            }
            K::Slice { recv, lo, hi } => {
                let rt = self.expr_one(recv, None);
                for b in [lo, hi] {
                    let t = self.expr_one(b, Some(&Type::Int));
                    if !matches!(t, Type::Int | Type::Unknown) {
                        self.diag(b.line, b.col, format!("slice bound must be int, got {t}"));
                    }
                }
                match rt {
                    Type::List(_) | Type::Str | Type::Unknown => one(rt),
                    t => {
                        self.diag(line, col, format!("cannot slice {t}"));
                        one(Type::Unknown)
                    }
                }
            }
            K::Lambda { params, ret, body } => one(self.lambda(params, ret, body, expected, line, col)),
            K::Check(inner) => {
                if self.current_ret.last() != Some(&err_opt()) {
                    self.diag(line, col, "check requires enclosing function to return error?");
                }
                let ty = self.check_expr(inner, None);
                let parts = match ty {
                    ExprTy::One(t) => vec![t],
                    ExprTy::Multi(ts) => ts,
                    ExprTy::PyChain => vec![Type::Py, err_opt()],
                };
                if parts.last() != Some(&err_opt()) && parts.last() != Some(&Type::Unknown) {
                    self.diag(line, col, "check needs a fallible expression");
                    return one(Type::Unknown);
                }
                let rest = &parts[..parts.len().saturating_sub(1)];
                match rest.len() {
                    0 => one(Type::Unit),
                    1 => one(rest[0].clone()),
                    _ => ExprTy::Multi(rest.to_vec()),
                }
            }
            K::Conv { target, arg } => {
                let t = self.resolve(target, line, col);
                let at = self.expr_pyish(arg, None);
                let ok = match (&t, &at) {
                    (_, Type::Py) | (_, Type::Unknown) => true,
                    (Type::Int, Type::Int | Type::Float | Type::Str) => true,
                    (Type::Float, Type::Int | Type::Float | Type::Str) => true,
                    (Type::Bool, Type::Str | Type::Bool) => true,
                    (Type::Str, _) => true,
                    (Type::List(_), Type::List(_)) => true,
                    _ => false,
                };
                if !ok {
                    self.diag(line, col, format!("cannot convert {at} to {t}"));
                }
                ExprTy::Multi(vec![t, err_opt()])
            }
        }
    }

    fn binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, line: u32, col: u32) -> Type {
        let lt = self.expr_pyish(lhs, None);
        let rt = self.expr_pyish(rhs, Some(&lt));
        if lt == Type::Py || rt == Type::Py {
            if matches!(op, BinOp::And | BinOp::Or) {
                self.diag(line, col, "&& and || need bool, got py");
                return Type::Unknown;
            }
            return Type::Py;
        }
        let unknown = lt == Type::Unknown || rt == Type::Unknown;
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                if unknown {
                    return Type::Unknown;
                }
                if (lt == Type::Int && rt == Type::Float) || (lt == Type::Float && rt == Type::Int)
                {
                    self.diag(line, col, "int and float do not mix");
                    return Type::Unknown;
                }
                match (&lt, &rt, op) {
                    (Type::Int, Type::Int, _) => Type::Int,
                    (Type::Float, Type::Float, BinOp::Rem) => {
                        self.diag(line, col, "% needs int operands");
                        Type::Unknown
                    }
                    (Type::Float, Type::Float, _) => Type::Float,
                    (Type::Str, Type::Str, BinOp::Add) => Type::Str,
                    (Type::List(a), Type::List(b), BinOp::Add) => {
                        // the wider element type wins, so an option side
                        // cannot hide behind a plain one
                        if a.accepts(b) {
                            lt.clone()
                        } else if b.accepts(a) {
                            rt.clone()
                        } else {
                            self.diag(line, col, format!("cannot concat list[{a}] and list[{b}]"));
                            lt.clone()
                        }
                    }
                    _ => {
                        self.diag(line, col, format!("cannot apply operator to {lt} and {rt}"));
                        Type::Unknown
                    }
                }
            }
            BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                if !unknown {
                    let ok = matches!(
                        (&lt, &rt),
                        (Type::Int, Type::Int) | (Type::Float, Type::Float) | (Type::Str, Type::Str)
                    );
                    if !ok {
                        self.diag(line, col, format!("cannot compare {lt} and {rt}"));
                    }
                }
                Type::Bool
            }
            BinOp::Eq | BinOp::NotEq => {
                let l_none = matches!(lhs.kind, ExprKind::NoneLit);
                let r_none = matches!(rhs.kind, ExprKind::NoneLit);
                if l_none || r_none {
                    let other = if l_none { &rt } else { &lt };
                    if !matches!(other, Type::Opt(_) | Type::Unknown) {
                        self.diag(line, col, "none only compares to option types");
                    }
                } else if !unknown {
                    let comparable = matches!(
                        &lt,
                        Type::Int | Type::Float | Type::Str | Type::Bool
                    ) && lt == rt;
                    if !comparable {
                        self.diag(line, col, format!("cannot compare {lt} and {rt}"));
                    }
                }
                Type::Bool
            }
            BinOp::And | BinOp::Or => {
                for (t, e) in [(&lt, lhs), (&rt, rhs)] {
                    if !matches!(t, Type::Bool | Type::Unknown) {
                        self.diag(e.line, e.col, format!("&& and || need bool, got {t}"));
                    }
                }
                Type::Bool
            }
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], line: u32, col: u32) -> ExprTy {
        // builtins by name
        if let ExprKind::Ident(name) = &callee.kind {
            if self.lookup(name).is_none() && !self.fns.contains_key(name) {
                match name.as_str() {
                    "print" => {
                        for a in args {
                            self.expr_one(a, None);
                        }
                        return ExprTy::One(Type::Unit);
                    }
                    "printf" | "sprintf" => {
                        if args.is_empty() {
                            self.diag(line, col, format!("{name} needs a format string"));
                        } else {
                            let t = self.expr_one(&args[0], Some(&Type::Str));
                            if !matches!(t, Type::Str | Type::Unknown) {
                                self.diag(line, col, format!("{name} format must be str, got {t}"));
                            }
                            let arg_tys: Vec<Type> =
                                args[1..].iter().map(|a| self.expr_one(a, None)).collect();
                            // a literal format is verified here; anything
                            // else stays a runtime check
                            if let ExprKind::Str(fmt) = &args[0].kind {
                                self.check_format(name, fmt, &arg_tys, line, col);
                            }
                        }
                        return ExprTy::One(if name == "sprintf" { Type::Str } else { Type::Unit });
                    }
                    "len" => {
                        if args.len() != 1 {
                            self.diag(line, col, "len takes one argument");
                        } else {
                            let t = self.expr_one(&args[0], None);
                            if !matches!(
                                t,
                                Type::Str | Type::List(_) | Type::Map(..) | Type::Unknown
                            ) {
                                self.diag(line, col, format!("len needs str, list, or map, got {t}"));
                            }
                        }
                        return ExprTy::One(Type::Int);
                    }
                    "range" => {
                        if args.is_empty() || args.len() > 2 {
                            self.diag(line, col, "range takes one or two arguments");
                        }
                        for a in args {
                            let t = self.expr_one(a, Some(&Type::Int));
                            if !matches!(t, Type::Int | Type::Unknown) {
                                self.diag(a.line, a.col, format!("range needs int, got {t}"));
                            }
                        }
                        return ExprTy::One(Type::List(Box::new(Type::Int)));
                    }
                    _ => {}
                }
            }
        }
        let ct = self.expr_pyish(callee, None);
        if ct == Type::Py {
            for a in args {
                self.expr_one(a, None);
            }
            return ExprTy::PyChain;
        }
        match ct {
            Type::Fn(params, rets) => {
                self.check_args(&params, args, line, col);
                rets_ty(rets)
            }
            Type::Unknown => ExprTy::One(Type::Unknown),
            t => {
                self.diag(line, col, format!("not callable: {t}"));
                ExprTy::One(Type::Unknown)
            }
        }
    }

    /// Statically checks a literal printf/sprintf format against the
    /// argument types. Mirrors the runtime verb table.
    fn check_format(&mut self, name: &str, fmt: &str, arg_tys: &[Type], line: u32, col: u32) {
        let mut verbs = vec![];
        let mut chars = fmt.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '%' {
                continue;
            }
            if chars.peek() == Some(&'%') {
                chars.next();
                continue;
            }
            // width and precision
            while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                chars.next();
            }
            if chars.peek() == Some(&'.') {
                chars.next();
                while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                    chars.next();
                }
            }
            match chars.next() {
                Some(v) => verbs.push(v),
                None => {
                    self.diag(line, col, format!("{name}: format ends inside a verb"));
                    return;
                }
            }
        }
        if verbs.len() != arg_tys.len() {
            self.diag(
                line,
                col,
                format!(
                    "{name}: wrong argument count ({} verbs, {} args)",
                    verbs.len(),
                    arg_tys.len()
                ),
            );
            return;
        }
        for (v, t) in verbs.iter().zip(arg_tys) {
            if *t == Type::Unknown {
                continue;
            }
            let want = match v {
                'v' => continue,
                'd' => Type::Int,
                's' | 'q' => Type::Str,
                't' => Type::Bool,
                'f' => Type::Float,
                _ => {
                    self.diag(line, col, format!("{name}: unknown verb %{v}"));
                    continue;
                }
            };
            if *t != want {
                self.diag(line, col, format!("{name}: %{v} needs {want}, got {t}"));
            }
        }
    }

    fn check_args(&mut self, params: &[Type], args: &[Expr], line: u32, col: u32) {
        if params.len() != args.len() {
            self.diag(line, col, format!("expected {} arguments, got {}", params.len(), args.len()));
        }
        for (p, a) in params.iter().zip(args) {
            let t = self.expr_one(a, Some(p));
            if !p.accepts(&t) {
                self.diag(a.line, a.col, format!("expected {p}, got {t}"));
            }
        }
    }

    fn method(&mut self, recv: &Expr, name: &str, args: &[Expr], line: u32, col: u32) -> ExprTy {
        // error.new / error.wrap: `error` is a type name acting as a module
        if let ExprKind::Ident(id) = &recv.kind {
            if id == "error" && self.lookup(id).is_none() {
                return match name {
                    "new" => {
                        self.check_args(&[Type::Str], args, line, col);
                        ExprTy::One(Type::Error)
                    }
                    "wrap" => {
                        self.check_args(&[Type::Error, Type::Str], args, line, col);
                        ExprTy::One(Type::Error)
                    }
                    _ => {
                        self.diag(line, col, format!("error has no member {name}"));
                        ExprTy::One(Type::Unknown)
                    }
                };
            }
        }
        let rt = self.expr_pyish(recv, None);
        if rt == Type::Py {
            for a in args {
                self.expr_one(a, None);
            }
            return ExprTy::PyChain;
        }
        match &rt {
            Type::Module(m) if matches!(self.imports.get(m), Some(ImportKind::File(_))) => {
                let mangled = format!("{m}.{name}");
                match self.fns.get(&mangled).cloned() {
                    Some((params, rets)) => {
                        self.check_args(&params, args, line, col);
                        rets_ty(rets)
                    }
                    None => {
                        self.diag(line, col, format!("{m} has no member {name}"));
                        ExprTy::One(Type::Unknown)
                    }
                }
            }
            Type::Module(m) => {
                // polymorphic math members
                if m == "math" && matches!(name, "abs" | "min" | "max") {
                    let want = if name == "abs" { 1 } else { 2 };
                    if args.len() != want {
                        self.diag(line, col, format!("math.{name} takes {want} arguments"));
                        return ExprTy::One(Type::Unknown);
                    }
                    let t0 = self.expr_one(&args[0], None);
                    if !matches!(t0, Type::Int | Type::Float | Type::Unknown) {
                        self.diag(line, col, format!("math.{name} needs int or float, got {t0}"));
                    }
                    for a in &args[1..] {
                        let t = self.expr_one(a, Some(&t0));
                        if !t0.accepts(&t) {
                            self.diag(a.line, a.col, format!("expected {t0}, got {t}"));
                        }
                    }
                    return ExprTy::One(t0);
                }
                match std_member(m, name) {
                    Some(Member::Fn(params, rets)) => {
                        self.check_args(&params, args, line, col);
                        rets_ty(rets)
                    }
                    Some(Member::Const(_)) => {
                        self.diag(line, col, format!("{m}.{name} is not callable"));
                        ExprTy::One(Type::Unknown)
                    }
                    None => {
                        self.diag(line, col, format!("{m} has no member {name}"));
                        ExprTy::One(Type::Unknown)
                    }
                }
            }
            Type::Struct(s) if s == "Ctx" => match name {
                "done" => {
                    self.check_args(&[], args, line, col);
                    ExprTy::One(Type::Bool)
                }
                "err" => {
                    self.check_args(&[], args, line, col);
                    ExprTy::One(err_opt())
                }
                _ => {
                    self.diag(line, col, format!("Ctx has no method {name}"));
                    ExprTy::One(Type::Unknown)
                }
            },
            Type::Str | Type::List(_) | Type::Map(..) => {
                self.container_method(&rt, name, args, line, col)
            }
            Type::Opt(_) => {
                self.diag(line, col, format!("value might be none; check it before calling {name}"));
                ExprTy::One(Type::Unknown)
            }
            Type::Unknown => ExprTy::One(Type::Unknown),
            t => {
                self.diag(line, col, format!("{t} has no method {name}"));
                ExprTy::One(Type::Unknown)
            }
        }
    }

    fn container_method(
        &mut self,
        recv: &Type,
        name: &str,
        args: &[Expr],
        line: u32,
        col: u32,
    ) -> ExprTy {
        use Type::*;
        let one = ExprTy::One;
        match (recv, name) {
            (Str, "split") => {
                self.check_args(&[Str], args, line, col);
                one(List(Box::new(Str)))
            }
            (Str, "trim" | "upper" | "lower") => {
                self.check_args(&[], args, line, col);
                one(Str)
            }
            (Str, "contains" | "starts_with" | "ends_with") => {
                self.check_args(&[Str], args, line, col);
                one(Bool)
            }
            (Str, "replace") => {
                self.check_args(&[Str, Str], args, line, col);
                one(Str)
            }
            (List(elem), "map") => {
                let f = Fn(vec![(**elem).clone()], vec![Unknown]);
                let got = self.args_with_fn(&f, args, line, col);
                let out = match got {
                    Some(Fn(_, rets)) if rets.len() == 1 => rets[0].clone(),
                    _ => Unknown,
                };
                one(List(Box::new(out)))
            }
            (List(elem), "filter") => {
                let f = Fn(vec![(**elem).clone()], vec![Bool]);
                self.args_with_fn(&f, args, line, col);
                one(recv.clone())
            }
            (List(elem), "each") => {
                let f = Fn(vec![(**elem).clone()], vec![]);
                self.args_with_fn(&f, args, line, col);
                one(Unit)
            }
            (List(elem), "sum") => {
                self.check_args(&[], args, line, col);
                if !matches!(**elem, Int | Float | Unknown) {
                    self.diag(line, col, format!("sum needs list[int] or list[float], got {recv}"));
                }
                one((**elem).clone())
            }
            (List(elem), "sorted") => {
                self.check_args(&[], args, line, col);
                if !matches!(**elem, Int | Float | Str | Unknown) {
                    self.diag(line, col, format!("sorted needs comparable elements, got {recv}"));
                }
                one(recv.clone())
            }
            (List(elem), "sorted_by") => {
                let f = Fn(vec![(**elem).clone(), (**elem).clone()], vec![Bool]);
                self.args_with_fn(&f, args, line, col);
                one(recv.clone())
            }
            (List(elem), "append") => {
                self.check_args(&[(**elem).clone()], args, line, col);
                one(recv.clone())
            }
            (List(elem), "contains") => {
                self.check_args(&[(**elem).clone()], args, line, col);
                one(Bool)
            }
            (List(elem), "join") => {
                if !matches!(**elem, Str | Unknown) {
                    self.diag(line, col, format!("join needs list[str], got {recv}"));
                }
                self.check_args(&[Str], args, line, col);
                one(Str)
            }
            (Map(k, _), "keys") => {
                self.check_args(&[], args, line, col);
                one(List(k.clone()))
            }
            (Map(_, v), "values") => {
                self.check_args(&[], args, line, col);
                one(List(v.clone()))
            }
            (Map(k, _), "has") => {
                self.check_args(&[(**k).clone()], args, line, col);
                one(Bool)
            }
            (Map(k, _), "delete") => {
                self.check_args(&[(**k).clone()], args, line, col);
                one(recv.clone())
            }
            _ => {
                self.diag(line, col, format!("{recv} has no method {name}"));
                one(Unknown)
            }
        }
    }

    /// Check args against a single expected fn param; returns the arg's
    /// (possibly inferred) type so callers can read the lambda's return.
    fn args_with_fn(&mut self, f: &Type, args: &[Expr], line: u32, col: u32) -> Option<Type> {
        if args.len() != 1 {
            self.diag(line, col, "expected one function argument");
            return None;
        }
        let t = self.expr_one(&args[0], Some(f));
        if let Type::Fn(want, _) = f {
            if let Type::Fn(got, _) = &t {
                if want.len() != got.len() {
                    self.diag(line, col, format!("expected {f}, got {t}"));
                }
            } else if t != Type::Unknown {
                self.diag(line, col, format!("expected {f}, got {t}"));
            }
        }
        Some(t)
    }

    fn field(&mut self, recv: &Expr, name: &str, line: u32, col: u32) -> Type {
        let rt = self.expr_pyish(recv, None);
        if rt == Type::Py {
            return Type::Py;
        }
        match &rt {
            Type::Struct(s) => match self.structs.get(s).and_then(|fs| {
                fs.iter().find(|(f, _)| f == name).map(|(_, t)| t.clone())
            }) {
                Some(t) => t,
                None => {
                    self.diag(line, col, format!("{s} has no field {name}"));
                    Type::Unknown
                }
            },
            Type::Error => match name {
                "msg" | "pytype" | "traceback" => Type::Str,
                "cause" => err_opt(),
                _ => {
                    self.diag(line, col, format!("error has no field {name}"));
                    Type::Unknown
                }
            },
            Type::Module(m) if matches!(self.imports.get(m), Some(ImportKind::File(_))) => {
                match self.fns.get(&format!("{m}.{name}")) {
                    Some((args, rets)) => Type::Fn(args.clone(), rets.clone()),
                    None => {
                        self.diag(line, col, format!("{m} has no member {name}"));
                        Type::Unknown
                    }
                }
            }
            Type::Module(m) => match std_member(m, name) {
                Some(Member::Const(t)) => t,
                Some(Member::Fn(args, rets)) => Type::Fn(args, rets),
                None => {
                    self.diag(line, col, format!("{m} has no member {name}"));
                    Type::Unknown
                }
            },
            Type::Opt(_) => {
                self.diag(line, col, format!("value might be none; check it before using .{name}"));
                Type::Unknown
            }
            Type::Unknown => Type::Unknown,
            t => {
                self.diag(line, col, format!("{t} has no field {name}"));
                Type::Unknown
            }
        }
    }

    fn lambda(
        &mut self,
        params: &[Param],
        ret: &Option<Vec<TypeExpr>>,
        body: &Block,
        expected: Option<&Type>,
        line: u32,
        col: u32,
    ) -> Type {
        let expected_fn = match expected {
            Some(Type::Fn(a, r)) => Some((a.clone(), r.clone())),
            _ => None,
        };
        let mut param_tys = vec![];
        for (i, p) in params.iter().enumerate() {
            let t = match &p.ty {
                Some(t) => self.resolve(t, line, col),
                None => match expected_fn.as_ref().and_then(|(a, _)| a.get(i)) {
                    Some(t) => t.clone(),
                    None => {
                        self.diag(line, col, format!("lambda parameter {} needs a type here", p.name));
                        Type::Unknown
                    }
                },
            };
            param_tys.push(t);
        }
        let declared_ret: Option<Vec<Type>> =
            ret.as_ref().map(|rs| rs.iter().map(|t| self.resolve(t, line, col)).collect());

        let saved_ret = std::mem::take(&mut self.current_ret);
        let saved_loop = std::mem::replace(&mut self.loop_depth, 0);
        self.push_scope();
        for (p, t) in params.iter().zip(&param_tys) {
            self.declare(&p.name, t.clone(), line, col);
        }

        let ret_tys: Vec<Type> = if let Some(rs) = declared_ret {
            self.current_ret = rs.clone();
            let diverges = self.check_block_inline(body);
            if !rs.is_empty() && !diverges {
                self.diag(line, col, "missing return in lambda");
            }
            rs
        } else if body.len() == 1 {
            // expression body: fn(x) { x > 2 }
            match &body[0].kind {
                StmtKind::Expr(e) => {
                    let t = self.expr_one(e, None);
                    if t == Type::Unit {
                        vec![]
                    } else {
                        vec![t]
                    }
                }
                _ => {
                    self.current_ret = vec![];
                    self.check_block_inline(body);
                    vec![]
                }
            }
        } else {
            self.current_ret = vec![];
            self.check_block_inline(body);
            vec![]
        };

        self.pop_scope();
        self.current_ret = saved_ret;
        self.loop_depth = saved_loop;
        Type::Fn(param_tys, ret_tys)
    }
}

fn rets_ty(rets: Vec<Type>) -> ExprTy {
    match rets.len() {
        0 => ExprTy::One(Type::Unit),
        1 => ExprTy::One(rets.into_iter().next().unwrap()),
        _ => ExprTy::Multi(rets),
    }
}

/// Every variable name assigned anywhere in the block, nested blocks included.
fn assigned_idents(b: &Block, out: &mut Vec<String>) {
    for s in b {
        match &s.kind {
            StmtKind::Assign { target, .. } => {
                if let ExprKind::Ident(n) = &target.kind {
                    out.push(n.clone());
                }
            }
            StmtKind::If { then, elifs, els, .. } => {
                assigned_idents(then, out);
                for (_, b) in elifs {
                    assigned_idents(b, out);
                }
                if let Some(b) = els {
                    assigned_idents(b, out);
                }
            }
            StmtKind::ForIn { body, .. } | StmtKind::ForCond { body, .. } => {
                assigned_idents(body, out);
            }
            _ => {}
        }
    }
}

fn contains_break(b: &Block) -> bool {
    b.iter().any(|s| match &s.kind {
        StmtKind::Break => true,
        StmtKind::If { then, elifs, els, .. } => {
            contains_break(then)
                || elifs.iter().any(|(_, b)| contains_break(b))
                || els.as_ref().is_some_and(|b| contains_break(b))
        }
        // breaks inside nested loops belong to those loops
        _ => false,
    })
}
