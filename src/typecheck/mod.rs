//! The static checking pass, run by `rikki check` and before every run.
//! Terminology: "check" here is this pass; the language's `check` expression
//! (ExprKind::Check) is a construct this pass validates like any other.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::{Diag, Span};
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
    current_file: Option<String>,
    loop_depth: u32,
    /// Scope depths at each enclosing function-literal boundary; names
    /// resolving below the innermost are captured copies.
    lambda_bases: Vec<usize>,
    diags: Vec<Diag>,
}

#[derive(Clone, PartialEq)]
enum ImportKind {
    Py,
    Std(String),
    File(String),
}

mod expr;
mod sigs;
use sigs::*;

impl Checker {
    fn diag(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diag {
            msg: msg.into(),
            span: Some(span),
            file: self.current_file.clone(),
        });
    }

    // ---------- collection pass ----------

    fn collect(&mut self, prog: &Program) {
        for d in &prog.decls {
            if let Decl::Import { path, py, .. } = d {
                if *py {
                    self.imports.insert(path.clone(), ImportKind::Py);
                } else if STD_MODULES.contains(&path.as_str()) {
                    self.imports
                        .insert(path.clone(), ImportKind::Std(path.clone()));
                    if path == "http" {
                        self.structs.extend(crate::stdlib::http::struct_types());
                    }
                    if path == "ctx" {
                        self.structs.insert("Ctx".into(), vec![]);
                    }
                }
                // file imports resolve after fns and structs are collected
            }
        }
        for d in &prog.decls {
            self.current_file = d.file().map(str::to_string);
            if let Decl::Struct { name, span, .. } = d {
                if self.structs.contains_key(name) {
                    self.diag(*span, format!("duplicate struct: {name}"));
                    continue;
                }
                self.structs.insert(name.clone(), vec![]);
            }
        }
        for d in &prog.decls {
            self.current_file = d.file().map(str::to_string);
            if let Decl::Struct {
                name, fields, span, ..
            } = d
            {
                let fs: Vec<(String, Type)> = fields
                    .iter()
                    .map(|(f, t)| (f.clone(), self.resolve(t, *span)))
                    .collect();
                self.structs.insert(name.clone(), fs);
            }
        }
        // a by-value field cycle can never be constructed; an option, list,
        // or map along the way breaks the cycle
        for d in &prog.decls {
            self.current_file = d.file().map(str::to_string);
            if let Decl::Struct { name, span, .. } = d {
                let cycle =
                    self.structs
                        .get(name)
                        .into_iter()
                        .flatten()
                        .find_map(|(f, t)| match t {
                            Type::Struct(s) if s == name || self.reaches_by_value(s, name) => {
                                Some((f.clone(), s.clone()))
                            }
                            _ => None,
                        });
                if let Some((field, fty)) = cycle {
                    self.diag(
                        *span,
                        format!("recursive struct {name}: use an option ({field}: {fty}?)"),
                    );
                }
            }
        }
        for d in &prog.decls {
            self.current_file = d.file().map(str::to_string);
            if let Decl::Fn(f) = d {
                let mut params = vec![];
                for p in &f.params {
                    match &p.ty {
                        Some(t) => params.push(self.resolve(t, f.span)),
                        None => {
                            self.diag(f.span, format!("parameter {} needs a type", p.name));
                            params.push(Type::Unknown);
                        }
                    }
                }
                let rets = f.ret.iter().map(|t| self.resolve(t, f.span)).collect();
                if self.fns.insert(f.name.clone(), (params, rets)).is_some() {
                    self.diag(f.span, format!("duplicate function: {}", f.name));
                }
            }
        }
        // second import pass: a path that isn't stdlib or py is a file import
        // if the merged program has symbols under its namespace
        for d in &prog.decls {
            self.current_file = d.file().map(str::to_string);
            if let Decl::Import {
                path,
                py: false,
                span,
                ..
            } = d
            {
                if STD_MODULES.contains(&path.as_str()) {
                    continue;
                }
                let prefix = crate::loader::qualified(path, "");
                let has_symbols = self.fns.keys().any(|k| k.starts_with(&prefix))
                    || self.structs.keys().any(|k| k.starts_with(&prefix));
                if has_symbols {
                    self.imports
                        .insert(path.clone(), ImportKind::File(path.clone()));
                } else {
                    self.diag(*span, format!("unknown module: {path}"));
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

    fn resolve(&mut self, t: &TypeExpr, span: Span) -> Type {
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
                        self.diag(span, format!("unknown type: {other}"));
                        Type::Unknown
                    }
                }
            },
            TypeExpr::List(inner) => Type::List(Box::new(self.resolve(inner, span))),
            TypeExpr::Map(k, v) => {
                let kt = self.resolve(k, span);
                if !matches!(kt, Type::Int | Type::Str | Type::Bool | Type::Unknown) {
                    self.diag(
                        span,
                        format!("map key type must be int, str, or bool, got {kt}"),
                    );
                }
                Type::Map(Box::new(kt), Box::new(self.resolve(v, span)))
            }
            TypeExpr::Opt(inner) => Type::Opt(Box::new(self.resolve(inner, span))),
            TypeExpr::Fn(args, rets) => Type::Fn(
                args.iter().map(|a| self.resolve(a, span)).collect(),
                rets.iter().map(|r| self.resolve(r, span)).collect(),
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

    /// Whether `name` resolves outside the innermost function literal:
    /// its capture is a by-value copy, so writes to it are silently lost.
    fn is_captured(&self, name: &str) -> bool {
        let Some(&base) = self.lambda_bases.last() else {
            return false;
        };
        match self.scopes.iter().rposition(|s| s.vars.contains_key(name)) {
            Some(d) => d < base,
            None => false,
        }
    }

    fn declared(&self, name: &str) -> Option<Type> {
        for s in self.scopes.iter().rev() {
            if let Some(t) = s.vars.get(name) {
                return Some(t.clone());
            }
        }
        None
    }

    /// The innermost scope. Every checking path pushes one first; self-heal
    /// with a fresh scope rather than panic if a future path forgets.
    fn top(&mut self) -> &mut Scope {
        if self.scopes.is_empty() {
            self.scopes.push(Scope::default());
        }
        let i = self.scopes.len() - 1;
        &mut self.scopes[i]
    }

    fn declare(&mut self, name: &str, ty: Type, span: Span) {
        if name == "_" {
            return;
        }
        if self.top().vars.contains_key(name) {
            self.diag(span, format!("already declared: {name}"));
        }
        self.top().vars.insert(name.to_string(), ty);
    }

    fn refine(&mut self, name: &str, ty: Type) {
        self.top().refits.insert(name.to_string(), ty);
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
            None => self.diag(Span::new(1, 1), "missing fn main"),
            Some((params, rets)) => {
                let ok = params.is_empty() && (rets.is_empty() || rets == &[err_opt()]);
                if !ok {
                    self.diag(
                        Span::new(1, 1),
                        "fn main takes no parameters and returns nothing or (error?)",
                    );
                }
            }
        }
        for d in &prog.decls {
            self.current_file = d.file().map(str::to_string);
            if let Decl::Fn(f) = d {
                self.check_fn(f);
            }
        }
    }

    fn check_fn(&mut self, f: &FnDecl) {
        self.current_ret = self
            .fns
            .get(&f.name)
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        self.push_scope();
        let param_tys: Vec<Type> = self
            .fns
            .get(&f.name)
            .map(|(p, _)| p.clone())
            .unwrap_or_default();
        for (p, t) in f.params.iter().zip(param_tys) {
            self.declare(&p.name, t, f.span);
        }
        let diverges = self.check_block(&f.body);
        self.pop_scope();
        if !self.current_ret.is_empty() && !diverges {
            self.diag(f.span, format!("missing return in {}", f.name));
        }
    }

    /// Checks a block in a fresh scope; returns whether it always diverges.
    fn check_block(&mut self, b: &Block) -> bool {
        self.push_scope();
        let mut diverges = false;
        for s in b {
            if diverges {
                self.diag(s.span, "unreachable code");
                break;
            }
            diverges = self.check_stmt(s);
        }
        self.pop_scope();
        diverges
    }

    // ---------- statements ----------

    fn check_stmt(&mut self, s: &Stmt) -> bool {
        let span = s.span;
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
                            self.diag(span, format!("{n} would have no value"));
                        }
                        self.declare(n, t, span);
                    }
                } else if names.len() == supplied.len() - 1 && supplied.last() == Some(&err_opt()) {
                    self.diag(span, "error result must be handled");
                } else {
                    self.diag(
                        span,
                        format!("expected {} values, got {}", names.len(), supplied.len()),
                    );
                    for n in names {
                        self.declare(n, Type::Unknown, span);
                    }
                }
                false
            }
            StmtKind::Assign { target, expr } => {
                let target_ty = match &target.kind {
                    ExprKind::Ident(n) => match self.declared(n) {
                        Some(t) => {
                            if self.is_captured(n) {
                                self.diag(
                                    span,
                                    format!("{n} is captured by value; writes inside a function literal do not escape"),
                                );
                            }
                            // assignment invalidates any narrowing
                            self.invalidate(n);
                            t
                        }
                        None => {
                            self.diag(span, format!("undefined: {n}"));
                            Type::Unknown
                        }
                    },
                    ExprKind::Index { .. } | ExprKind::Field { .. } => {
                        match self.check_expr(target, None) {
                            ExprTy::PyChain => {
                                // assignment into a py target: any value; the
                                // bridge's inbound table (13.5) governs at
                                // runtime, and an exception there faults
                                self.expr_one(expr, None);
                                return false;
                            }
                            ExprTy::One(t) => {
                                // mutating a captured copy is equally lost;
                                // py targets never reach here (chains above)
                                if let Some(n) = base_ident(target) {
                                    if self.is_captured(n) {
                                        self.diag(
                                            span,
                                            format!("{n} is captured by value; writes inside a function literal do not escape"),
                                        );
                                    }
                                }
                                match (&target.kind, t) {
                                // map read is V?, but assignment writes a V
                                (ExprKind::Index { recv, .. }, t) => {
                                    let recv_ty = self.expr_one(recv, None);
                                    match recv_ty {
                                        Type::Map(_, v) => *v,
                                        _ => t,
                                    }
                                }
                                    (_, t) => t,
                                }
                            }
                            ExprTy::Multi(_) => {
                                self.diag(span, "cannot assign to multiple values");
                                Type::Unknown
                            }
                        }
                    }
                    _ => {
                        self.diag(span, "cannot assign to this expression");
                        Type::Unknown
                    }
                };
                let val = self.expr_one(expr, Some(&target_ty));
                if !target_ty.accepts(&val) {
                    self.diag(span, format!("expected {target_ty}, got {val}"));
                }
                false
            }
            StmtKind::Expr(e) => {
                let ty = self.check_expr(e, None);
                match ty {
                    ExprTy::PyChain => {
                        self.diag(span, "error result must be handled");
                    }
                    ExprTy::Multi(ts) if ts.last() == Some(&err_opt()) => {
                        self.diag(span, "error result must be handled");
                    }
                    ExprTy::One(t) if t == err_opt() && !matches!(e.kind, ExprKind::Check(_)) => {
                        self.diag(span, "error result must be handled");
                    }
                    _ => {}
                }
                false
            }
            StmtKind::Return(exprs) => {
                let want = self.current_ret.clone();
                if exprs.is_empty() && !want.is_empty() {
                    // allow bare `return` only when everything has a zero... no: require values
                    self.diag(span, format!("expected {} return values", want.len()));
                } else if exprs.len() != want.len() {
                    self.diag(
                        span,
                        format!("expected {} return values, got {}", want.len(), exprs.len()),
                    );
                } else {
                    for (e, w) in exprs.iter().zip(&want) {
                        let t = self.expr_one(e, Some(w));
                        if !w.accepts(&t) {
                            self.diag(e.span, format!("expected {w}, got {t}"));
                        }
                    }
                }
                true
            }
            StmtKind::If {
                cond,
                then,
                elifs,
                els,
            } => {
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
            StmtKind::ForRange { names, iter, body } => {
                // ranging absorbs a py chain: iter() consumes it
                let it = self.expr_pyish(iter, None);
                self.push_scope();
                match (&it, names.len()) {
                    (Type::Int | Type::List(_) | Type::Map(..) | Type::Str | Type::Py, 0) => {}
                    (Type::Py, 1) => self.declare(&names[0], Type::Int, span),
                    (Type::Py, 2) => {
                        self.declare(&names[0], Type::Int, span);
                        self.declare(&names[1], Type::Py, span);
                    }
                    (Type::Int, 1) => self.declare(&names[0], Type::Int, span),
                    (Type::List(_) | Type::Str, 1) => self.declare(&names[0], Type::Int, span),
                    (Type::Str, 2) => {
                        self.declare(&names[0], Type::Int, span);
                        self.declare(&names[1], Type::Str, span);
                    }
                    (Type::List(t), 2) => {
                        self.declare(&names[0], Type::Int, span);
                        self.declare(&names[1], (**t).clone(), span);
                    }
                    (Type::Map(k, _), 1) => self.declare(&names[0], (**k).clone(), span),
                    (Type::Map(k, v), 2) => {
                        self.declare(&names[0], (**k).clone(), span);
                        self.declare(&names[1], (**v).clone(), span);
                    }
                    (Type::Unknown, _) => {
                        for n in names {
                            self.declare(n, Type::Unknown, span);
                        }
                    }
                    (_, 0) => self.diag(span, format!("cannot range over {it}")),
                    _ => {
                        let msg = if matches!(
                            it,
                            Type::Int | Type::List(_) | Type::Map(..) | Type::Str | Type::Py
                        ) {
                            format!("cannot range over {it} with {} names", names.len())
                        } else {
                            format!("cannot range over {it}")
                        };
                        self.diag(span, msg);
                        for n in names {
                            self.declare(n, Type::Unknown, span);
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
                    self.diag(span, "break or continue outside loop");
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
                self.diag(s.span, "unreachable code");
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
            self.diag(cond.span, format!("condition must be bool, got {t}"));
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
}

/// The identifier a field/index assignment chain bottoms out at.
fn base_ident(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Ident(n) => Some(n),
        ExprKind::Index { recv, .. } | ExprKind::Field { recv, .. } => base_ident(recv),
        _ => None,
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
            StmtKind::If {
                then, elifs, els, ..
            } => {
                assigned_idents(then, out);
                for (_, b) in elifs {
                    assigned_idents(b, out);
                }
                if let Some(b) = els {
                    assigned_idents(b, out);
                }
            }
            StmtKind::ForRange { body, .. } | StmtKind::ForCond { body, .. } => {
                assigned_idents(body, out);
            }
            _ => {}
        }
    }
}

fn contains_break(b: &Block) -> bool {
    b.iter().any(|s| match &s.kind {
        StmtKind::Break => true,
        StmtKind::If {
            then, elifs, els, ..
        } => {
            contains_break(then)
                || elifs.iter().any(|(_, b)| contains_break(b))
                || els.as_ref().is_some_and(contains_break)
        }
        // breaks inside nested loops belong to those loops
        _ => false,
    })
}
