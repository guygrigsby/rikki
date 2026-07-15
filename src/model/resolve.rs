//! Reference resolution: walks a loaded `Program`'s function bodies to find
//! references to module-level fns and structs, call edges, and py import
//! boundaries. See the model contract for the three `ReferenceForm`
//! semantics and the skeleton scope rule this walk relies on.

use std::collections::{HashMap, HashSet};

use crate::ast::{Block, Decl, Expr, ExprKind, Param, Program, Stmt, StmtKind};
use crate::diag::Span;
use crate::loader;

use super::symbols;
use super::{CallEdge, Pos, PyBoundary, Reference, ReferenceForm, SymbolId, SymbolKind};

/// Resolve `prog` into references, call edges, and py boundaries: one pass
/// per function body over every module-level fn (root or imported), plus a
/// pass over top-level `import py` declarations.
pub fn resolve(prog: &Program) -> (Vec<Reference>, Vec<CallEdge>, Vec<PyBoundary>) {
    let targets = build_targets(prog);
    let import_stems = build_import_stems(prog);

    let mut references = Vec::new();
    let mut calls = Vec::new();
    let mut py_boundaries = Vec::new();

    for decl in &prog.decls {
        match decl {
            Decl::Fn(f) => {
                let file = f.file.clone().unwrap_or_default();
                let caller = SymbolId(symbols::symbol_id(SymbolKind::Function, &file, &f.name));
                let mut locals = HashSet::new();
                collect_params(&f.params, &mut locals);
                collect_block_locals(&f.body, &mut locals);
                let empty = HashSet::new();
                let stems = import_stems.get(&file).unwrap_or(&empty);
                let mut walker = Walker {
                    targets: &targets,
                    stems,
                    locals: &locals,
                    file: &file,
                    caller: &caller,
                    references: &mut references,
                    calls: &mut calls,
                };
                walker.walk_block(&f.body);
            }
            Decl::Struct { .. } => {}
            Decl::Import {
                path,
                py: true,
                span,
                file,
            } => {
                py_boundaries.push(PyBoundary {
                    file: file.clone().unwrap_or_default(),
                    at: to_pos(*span),
                    note: format!("import py \"{path}\""),
                });
            }
            Decl::Import { py: false, .. } => {}
        }
    }

    (references, calls, py_boundaries)
}

/// Function and struct symbols keyed by their loader-qualified name: the
/// lookup table both `Ident` and `ModuleMemberCall` resolution consult.
fn build_targets(prog: &Program) -> HashMap<String, SymbolId> {
    symbols::extract(prog)
        .into_iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Struct))
        .map(|s| (s.qualified, s.id))
        .collect()
}

/// Per-file set of file-import stems (`import "util.nv"` -> `"util"`,
/// already rewritten to a bare stem by the loader). Only `py: false`
/// imports count; `import py` never introduces a module-member-call stem.
fn build_import_stems(prog: &Program) -> HashMap<String, HashSet<String>> {
    let mut out: HashMap<String, HashSet<String>> = HashMap::new();
    for decl in &prog.decls {
        if let Decl::Import {
            path,
            py: false,
            file,
            ..
        } = decl
        {
            out.entry(file.clone().unwrap_or_default())
                .or_default()
                .insert(path.clone());
        }
    }
    out
}

fn to_pos(span: Span) -> Pos {
    Pos {
        line: span.line,
        col: span.col,
    }
}

// --- locals collection: function-scoped over-approximation (contract's
// skeleton scope rule) --------------------------------------------------

fn collect_params(params: &[Param], locals: &mut HashSet<String>) {
    for p in params {
        locals.insert(p.name.clone());
    }
}

fn collect_block_locals(block: &Block, locals: &mut HashSet<String>) {
    for stmt in block {
        collect_stmt_locals(stmt, locals);
    }
}

fn collect_stmt_locals(stmt: &Stmt, locals: &mut HashSet<String>) {
    match &stmt.kind {
        StmtKind::Let { names, expr } => {
            for n in names {
                locals.insert(n.clone());
            }
            collect_expr_locals(expr, locals);
        }
        StmtKind::Assign { target, expr } => {
            collect_expr_locals(target, locals);
            collect_expr_locals(expr, locals);
        }
        StmtKind::Expr(e) => collect_expr_locals(e, locals),
        StmtKind::Return(es) => {
            for e in es {
                collect_expr_locals(e, locals);
            }
        }
        StmtKind::If {
            cond,
            then,
            elifs,
            els,
        } => {
            collect_expr_locals(cond, locals);
            collect_block_locals(then, locals);
            for (c, b) in elifs {
                collect_expr_locals(c, locals);
                collect_block_locals(b, locals);
            }
            if let Some(b) = els {
                collect_block_locals(b, locals);
            }
        }
        StmtKind::ForRange { names, iter, body } => {
            for n in names {
                locals.insert(n.clone());
            }
            collect_expr_locals(iter, locals);
            collect_block_locals(body, locals);
        }
        StmtKind::ForCond { cond, body } => {
            if let Some(c) = cond {
                collect_expr_locals(c, locals);
            }
            collect_block_locals(body, locals);
        }
        StmtKind::With { expr, body } => {
            collect_expr_locals(expr, locals);
            collect_block_locals(body, locals);
        }
        StmtKind::Break | StmtKind::Continue => {}
    }
}

fn collect_expr_locals(expr: &Expr, locals: &mut HashSet<String>) {
    match &expr.kind {
        ExprKind::Lambda { params, body, .. } => {
            collect_params(params, locals);
            collect_block_locals(body, locals);
        }
        ExprKind::Ident(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::NoneLit => {}
        ExprKind::List(items) => {
            for i in items {
                collect_expr_locals(i, locals);
            }
        }
        ExprKind::ListLit { items, .. } => {
            for i in items {
                collect_expr_locals(i, locals);
            }
        }
        ExprKind::MapLit { entries, .. } => {
            for (k, v) in entries {
                collect_expr_locals(k, locals);
                collect_expr_locals(v, locals);
            }
        }
        ExprKind::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_expr_locals(v, locals);
            }
        }
        ExprKind::Unary { rhs, .. } => collect_expr_locals(rhs, locals),
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_expr_locals(lhs, locals);
            collect_expr_locals(rhs, locals);
        }
        ExprKind::Call {
            callee,
            args,
            kwargs,
        } => {
            collect_expr_locals(callee, locals);
            for a in args {
                collect_expr_locals(a, locals);
            }
            for (_, v) in kwargs {
                collect_expr_locals(v, locals);
            }
        }
        ExprKind::Method {
            recv, args, kwargs, ..
        } => {
            collect_expr_locals(recv, locals);
            for a in args {
                collect_expr_locals(a, locals);
            }
            for (_, v) in kwargs {
                collect_expr_locals(v, locals);
            }
        }
        ExprKind::Field { recv, .. } => collect_expr_locals(recv, locals),
        ExprKind::Index { recv, idx } => {
            collect_expr_locals(recv, locals);
            collect_expr_locals(idx, locals);
        }
        ExprKind::Slice { recv, lo, hi } => {
            collect_expr_locals(recv, locals);
            collect_expr_locals(lo, locals);
            collect_expr_locals(hi, locals);
        }
        ExprKind::Check(inner) => collect_expr_locals(inner, locals),
        ExprKind::Conv { arg, .. } => collect_expr_locals(arg, locals),
    }
}

// --- the reference/call walk --------------------------------------------

struct Walker<'a> {
    targets: &'a HashMap<String, SymbolId>,
    stems: &'a HashSet<String>,
    locals: &'a HashSet<String>,
    file: &'a str,
    caller: &'a SymbolId,
    references: &'a mut Vec<Reference>,
    calls: &'a mut Vec<CallEdge>,
}

impl Walker<'_> {
    fn walk_block(&mut self, block: &Block) {
        for stmt in block {
            self.walk_stmt(stmt);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let { expr, .. } => self.walk_expr(expr),
            StmtKind::Assign { target, expr } => {
                self.walk_expr(target);
                self.walk_expr(expr);
            }
            StmtKind::Expr(e) => self.walk_expr(e),
            StmtKind::Return(es) => {
                for e in es {
                    self.walk_expr(e);
                }
            }
            StmtKind::If {
                cond,
                then,
                elifs,
                els,
            } => {
                self.walk_expr(cond);
                self.walk_block(then);
                for (c, b) in elifs {
                    self.walk_expr(c);
                    self.walk_block(b);
                }
                if let Some(b) = els {
                    self.walk_block(b);
                }
            }
            StmtKind::ForRange { iter, body, .. } => {
                self.walk_expr(iter);
                self.walk_block(body);
            }
            StmtKind::ForCond { cond, body } => {
                if let Some(c) = cond {
                    self.walk_expr(c);
                }
                self.walk_block(body);
            }
            StmtKind::With { expr, body } => {
                self.walk_expr(expr);
                self.walk_block(body);
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Ident(name) => self.resolve_ident(name, expr.span, false),
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::NoneLit => {}
            ExprKind::List(items) => {
                for i in items {
                    self.walk_expr(i);
                }
            }
            ExprKind::ListLit { items, .. } => {
                for i in items {
                    self.walk_expr(i);
                }
            }
            ExprKind::MapLit { entries, .. } => {
                for (k, v) in entries {
                    self.walk_expr(k);
                    self.walk_expr(v);
                }
            }
            ExprKind::StructLit { name, fields } => {
                if let Some(target) = self.targets.get(name) {
                    self.references.push(Reference {
                        target: target.clone(),
                        file: self.file.to_string(),
                        at: to_pos(expr.span),
                        form: ReferenceForm::StructLiteral,
                    });
                }
                for (_, v) in fields {
                    self.walk_expr(v);
                }
            }
            ExprKind::Unary { rhs, .. } => self.walk_expr(rhs),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.walk_expr(lhs);
                self.walk_expr(rhs);
            }
            ExprKind::Call {
                callee,
                args,
                kwargs,
            } => {
                self.walk_call_callee(callee);
                for a in args {
                    self.walk_expr(a);
                }
                for (_, v) in kwargs {
                    self.walk_expr(v);
                }
            }
            ExprKind::Method {
                recv,
                name,
                args,
                kwargs,
            } => {
                self.walk_method(recv, name, expr.span);
                for a in args {
                    self.walk_expr(a);
                }
                for (_, v) in kwargs {
                    self.walk_expr(v);
                }
            }
            ExprKind::Field { recv, .. } => self.walk_expr(recv),
            ExprKind::Index { recv, idx } => {
                self.walk_expr(recv);
                self.walk_expr(idx);
            }
            ExprKind::Slice { recv, lo, hi } => {
                self.walk_expr(recv);
                self.walk_expr(lo);
                self.walk_expr(hi);
            }
            ExprKind::Lambda { body, .. } => self.walk_block(body),
            ExprKind::Check(inner) => self.walk_expr(inner),
            ExprKind::Conv { arg, .. } => self.walk_expr(arg),
        }
    }

    /// The callee slot of a `Call`: an `Ident` callee is a candidate
    /// function reference (and, if it resolves, a call edge); anything else
    /// (a chained call, a lambda immediately invoked, ...) just walks
    /// normally.
    fn walk_call_callee(&mut self, callee: &Expr) {
        if let ExprKind::Ident(name) = &callee.kind {
            self.resolve_ident(name, callee.span, true);
        } else {
            self.walk_expr(callee);
        }
    }

    fn resolve_ident(&mut self, name: &str, at: Span, is_call: bool) {
        if self.locals.contains(name) {
            return;
        }
        let Some(target) = self.targets.get(name) else {
            return;
        };
        self.references.push(Reference {
            target: target.clone(),
            file: self.file.to_string(),
            at: to_pos(at),
            form: ReferenceForm::Ident,
        });
        if is_call {
            self.calls.push(CallEdge {
                caller: self.caller.clone(),
                callee: target.clone(),
                at: to_pos(at),
            });
        }
    }

    /// `recv.name(...)`: a module-member call when `recv` is a bare `Ident`
    /// naming a non-shadowed file-import stem and `stem.name` resolves. On a
    /// hit, `recv` is consumed here and must not also be walked as a plain
    /// `Ident` by the caller. On a miss (a value-method call like
    /// `xs.append(..)`, or a builtin-module call like `math.abs(..)` whose
    /// stem is not a file import), `recv` is walked normally.
    fn walk_method(&mut self, recv: &Expr, name: &str, method_span: Span) {
        if let ExprKind::Ident(m) = &recv.kind {
            if !self.locals.contains(m) && self.stems.contains(m) {
                let qualified = loader::qualified(m, name);
                if let Some(target) = self.targets.get(&qualified) {
                    self.references.push(Reference {
                        target: target.clone(),
                        file: self.file.to_string(),
                        at: to_pos(method_span),
                        form: ReferenceForm::ModuleMemberCall,
                    });
                    self.calls.push(CallEdge {
                        caller: self.caller.clone(),
                        callee: target.clone(),
                        at: to_pos(method_span),
                    });
                    return;
                }
            }
        }
        self.walk_expr(recv);
    }
}
