//! `nevla tidy`: the goimports half of formatting. fmt only lays out
//! (its contract is AST preservation); tidy edits the import set, then
//! renders through the formatter. It adds missing stdlib imports
//! (module receivers and stdlib-injected struct names both count as
//! use), removes unused imports of every kind, sorts the group (plain
//! paths, then py, alphabetical within), and merges it at the position
//! of the first import.
//!
//! Resolution is syntactic, not scoped: a local variable shadowing a
//! stdlib module name can fool it. The checker still arbitrates; tidy
//! output that fails `nevla check` is a bug report waiting to happen.

use std::collections::HashSet;

use crate::ast::*;
use crate::diag::Diag;

/// Stdlib modules whose import declares struct types: using such a type
/// is using the module. Built from the same struct_types functions the
/// checker injects from, so this cannot drift.
fn injected_struct_owners() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("ctx", vec!["Ctx".to_string()]),
        (
            "http",
            crate::stdlib::http::struct_types()
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
        ),
        (
            "time",
            crate::stdlib::time::struct_types()
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
        ),
        (
            "regex",
            crate::stdlib::regex::struct_types()
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
        ),
        (
            "flag",
            crate::stdlib::flag::struct_types()
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
        ),
        (
            "proc",
            crate::stdlib::proc::struct_types()
                .into_iter()
                .map(|(n, _)| n)
                .collect(),
        ),
    ]
}

/// Modules tidy may add on sight of a receiver. "error" is excluded:
/// its constructors need no import (spec 15.1), so the import is never
/// missing, and an existing one is left alone below.
const ADDABLE: &[&str] = &[
    "math", "file", "ctx", "gpu", "http", "test", "time", "os", "regex", "flag", "proc",
];

#[derive(Default)]
struct Usage {
    /// names in module-receiver position: `os.args()`, `math.pi`
    receivers: HashSet<String>,
    /// struct type names used in literals, annotations, or conversions
    types: HashSet<String>,
}

impl Usage {
    fn ty(&mut self, t: &TypeExpr) {
        match t {
            TypeExpr::Named(n) => {
                self.types.insert(n.clone());
            }
            TypeExpr::List(e) | TypeExpr::Opt(e) => self.ty(e),
            TypeExpr::Map(k, v) => {
                self.ty(k);
                self.ty(v);
            }
            TypeExpr::Fn(ps, rs) => {
                for t in ps.iter().chain(rs) {
                    self.ty(t);
                }
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Method { recv, args, kwargs, .. } => {
                if let ExprKind::Ident(n) = &recv.kind {
                    self.receivers.insert(n.clone());
                }
                self.expr(recv);
                for a in args {
                    self.expr(a);
                }
                for (_, a) in kwargs {
                    self.expr(a);
                }
            }
            ExprKind::Field { recv, .. } => {
                if let ExprKind::Ident(n) = &recv.kind {
                    self.receivers.insert(n.clone());
                }
                self.expr(recv);
            }
            ExprKind::StructLit { name, fields } => {
                self.types.insert(name.clone());
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            ExprKind::Conv { target, arg } => {
                self.ty(target);
                self.expr(arg);
            }
            ExprKind::List(items) => {
                for i in items {
                    self.expr(i);
                }
            }
            ExprKind::ListLit { elem, items } => {
                self.ty(elem);
                for i in items {
                    self.expr(i);
                }
            }
            ExprKind::MapLit { key, val, entries } => {
                self.ty(key);
                self.ty(val);
                for (k, v) in entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            ExprKind::Unary { rhs, .. } => self.expr(rhs),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Call { callee, args, kwargs } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
                for (_, a) in kwargs {
                    self.expr(a);
                }
            }
            ExprKind::Index { recv, idx } => {
                self.expr(recv);
                self.expr(idx);
            }
            ExprKind::Slice { recv, lo, hi } => {
                self.expr(recv);
                self.expr(lo);
                self.expr(hi);
            }
            ExprKind::Lambda { params, ret, body } => {
                for p in params {
                    if let Some(t) = &p.ty {
                        self.ty(t);
                    }
                }
                for t in ret.iter().flatten() {
                    self.ty(t);
                }
                self.block(body);
            }
            ExprKind::Check(inner) => self.expr(inner),
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::NoneLit
            | ExprKind::Ident(_) => {}
        }
    }

    fn block(&mut self, b: &Block) {
        for st in b {
            match &st.kind {
                StmtKind::Let { expr, .. } => self.expr(expr),
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
                StmtKind::If { cond, then, elifs, els } => {
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
                StmtKind::ForRange { iter, body, .. } => {
                    self.expr(iter);
                    self.block(body);
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
    }
}

/// The name an import binds (spec 6, 13.1, 16.1).
fn binds(path: &str, py: bool) -> String {
    if py {
        return path.split('.').next().unwrap_or(path).to_string();
    }
    match path.strip_suffix(".nv") {
        Some(stem) => stem.rsplit('/').next().unwrap_or(stem).to_string(),
        None => path.to_string(),
    }
}

pub fn tidy_source(src: &str) -> Result<String, Diag> {
    let prog = crate::parser::parse(src)?;

    let mut usage = Usage::default();
    for d in &prog.decls {
        match d {
            Decl::Fn(f) => {
                for p in &f.params {
                    if let Some(t) = &p.ty {
                        usage.ty(t);
                    }
                }
                for t in &f.ret {
                    usage.ty(t);
                }
                usage.block(&f.body);
            }
            Decl::Struct { fields, .. } => {
                for (_, t) in fields {
                    usage.ty(t);
                }
            }
            Decl::Import { .. } => {}
        }
    }

    let owners = injected_struct_owners();
    let module_used = |name: &str| {
        usage.receivers.contains(name)
            || owners
                .iter()
                .any(|(m, structs)| *m == name && structs.iter().any(|s| usage.types.contains(s)))
    };

    // keep used imports; "error" is never removed (its constructors need
    // no import, so presence is a deliberate statement)
    let mut kept: Vec<(String, bool)> = vec![];
    let mut first_import_line = 0u32;
    for d in &prog.decls {
        if let Decl::Import { path, py, span, .. } = d {
            if first_import_line == 0 {
                first_import_line = span.line;
            }
            let name = binds(path, *py);
            if path == "error" || module_used(&name) {
                if !kept.iter().any(|(p, b)| p == path && b == py) {
                    kept.push((path.clone(), *py));
                }
            }
        }
    }

    // add missing stdlib imports for receivers and injected struct types
    for m in ADDABLE {
        let imported = kept.iter().any(|(p, py)| !py && p == m);
        if !imported && module_used(m) {
            kept.push((m.to_string(), false));
        }
    }

    // plain paths first, then py, alphabetical within
    kept.sort_by(|a, b| (a.1, a.0.as_str()).cmp(&(b.1, b.0.as_str())));

    let line = if first_import_line == 0 { 1 } else { first_import_line };
    let imports: Vec<Decl> = kept
        .into_iter()
        .map(|(path, py)| Decl::Import {
            path,
            py,
            span: crate::diag::Span::new(line, 1),
            file: None,
        })
        .collect();

    // one group at the position of the first original import
    let mut decls: Vec<Decl> = vec![];
    let mut placed = imports.is_empty();
    for d in prog.decls.into_iter() {
        match d {
            Decl::Import { .. } => {
                if !placed {
                    decls.extend(imports.iter().cloned());
                    placed = true;
                }
            }
            other => {
                if !placed {
                    decls.extend(imports.iter().cloned());
                    placed = true;
                }
                decls.push(other);
            }
        }
    }
    if !placed {
        decls.extend(imports);
    }

    crate::format::fmt_program(&Program { decls }, src)
}
