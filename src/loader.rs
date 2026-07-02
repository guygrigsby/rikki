//! Multi-file loading. `import "util.mg"` pulls in a sibling file whose
//! top-level names are exposed under the file-stem namespace. Implementation:
//! parse each file once, rename its module-level symbols to `stem.name`
//! (scope-aware, so locals that shadow them survive), and merge everything
//! into one Program.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::diag::Diag;
use crate::parser;

pub fn load(path: &Path) -> Result<Program, Diag> {
    let mut loading = vec![];
    let mut loaded = HashSet::new();
    let mut merged = Program::default();
    load_file(path, true, &mut loading, &mut loaded, &mut merged)?;
    Ok(merged)
}

fn errd(msg: String) -> Diag {
    Diag { msg, line: 1, col: 1 }
}

fn load_file(
    path: &Path,
    is_root: bool,
    loading: &mut Vec<PathBuf>,
    loaded: &mut HashSet<PathBuf>,
    merged: &mut Program,
) -> Result<(), Diag> {
    let canon = path
        .canonicalize()
        .map_err(|e| errd(format!("cannot read import {}: {e}", path.display())))?;
    if loading.contains(&canon) {
        let names: Vec<String> = loading
            .iter()
            .chain([&canon])
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
            .collect();
        return Err(errd(format!("import cycle: {}", names.join(" -> "))));
    }
    if loaded.contains(&canon) {
        return Ok(());
    }
    let src = std::fs::read_to_string(&canon)
        .map_err(|e| errd(format!("cannot read import {}: {e}", path.display())))?;
    let mut prog = parser::parse(&src)?;

    loading.push(canon.clone());
    let dir = canon.parent().map(Path::to_path_buf).unwrap_or_default();
    for d in &prog.decls {
        if let Decl::Import { path: p, py: false, .. } = d {
            if p.ends_with(".mg") {
                load_file(&dir.join(p), false, loading, loaded, merged)?;
            }
        }
    }
    loading.pop();
    loaded.insert(canon.clone());

    if !is_root {
        let stem = canon
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .ok_or_else(|| errd(format!("bad module path: {}", canon.display())))?;
        rename_module(&mut prog, &stem);
    }
    // rewrite file imports to plain namespace markers understood downstream
    for d in &mut prog.decls {
        if let Decl::Import { path: p, py: false, .. } = d {
            if p.ends_with(".mg") {
                *p = Path::new(&p.clone())
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| p.clone());
            }
        }
    }
    merged.decls.append(&mut prog.decls);
    Ok(())
}

/// Prefix every module-level name (fns, structs) with `stem.` throughout the
/// module's own AST, respecting local shadowing.
fn rename_module(prog: &mut Program, stem: &str) {
    let mut decls = HashSet::new();
    for d in &prog.decls {
        match d {
            Decl::Fn(f) => {
                decls.insert(f.name.clone());
            }
            Decl::Struct { name, .. } => {
                decls.insert(name.clone());
            }
            Decl::Import { .. } => {}
        }
    }
    let mut r = Renamer { decls, prefix: stem.to_string(), locals: vec![] };
    for d in &mut prog.decls {
        match d {
            Decl::Fn(f) => {
                f.name = r.mangled(&f.name);
                r.locals.push(f.params.iter().map(|p| p.name.clone()).collect());
                for p in &mut f.params {
                    if let Some(t) = &mut p.ty {
                        r.ty(t);
                    }
                }
                for t in &mut f.ret {
                    r.ty(t);
                }
                r.block(&mut f.body);
                r.locals.pop();
            }
            Decl::Struct { name, fields, .. } => {
                *name = r.mangled(name);
                for (_, t) in fields {
                    r.ty(t);
                }
            }
            Decl::Import { .. } => {}
        }
    }
}

struct Renamer {
    decls: HashSet<String>,
    prefix: String,
    locals: Vec<HashSet<String>>,
}

impl Renamer {
    fn mangled(&self, name: &str) -> String {
        format!("{}.{name}", self.prefix)
    }

    fn hit(&self, name: &str) -> bool {
        self.decls.contains(name) && !self.locals.iter().any(|s| s.contains(name))
    }

    fn declare(&mut self, name: &str) {
        if let Some(s) = self.locals.last_mut() {
            s.insert(name.to_string());
        }
    }

    fn ty(&mut self, t: &mut TypeExpr) {
        match t {
            TypeExpr::Named(n) => {
                if self.hit(n) {
                    *n = self.mangled(n);
                }
            }
            TypeExpr::List(x) | TypeExpr::Opt(x) => self.ty(x),
            TypeExpr::Map(k, v) => {
                self.ty(k);
                self.ty(v);
            }
            TypeExpr::Fn(args, rets) => {
                for x in args.iter_mut().chain(rets.iter_mut()) {
                    self.ty(x);
                }
            }
        }
    }

    fn block(&mut self, b: &mut Block) {
        self.locals.push(HashSet::new());
        for s in b {
            self.stmt(s);
        }
        self.locals.pop();
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match &mut s.kind {
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
            StmtKind::ForIn { names, iter, body } => {
                self.expr(iter);
                self.locals.push(names.iter().cloned().collect());
                self.block(body);
                self.locals.pop();
            }
            StmtKind::ForCond { cond, body } => {
                if let Some(c) = cond {
                    self.expr(c);
                }
                self.block(body);
            }
            StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::Ident(n) => {
                if self.hit(n) {
                    *n = self.mangled(n);
                }
            }
            ExprKind::StructLit { name, fields } => {
                if self.hit(name) {
                    *name = self.mangled(name);
                }
                for (_, v) in fields {
                    self.expr(v);
                }
            }
            ExprKind::List(items) => {
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
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Method { recv, args, .. } => {
                self.expr(recv);
                for a in args {
                    self.expr(a);
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
            ExprKind::Lambda { params, ret, body } => {
                self.locals.push(params.iter().map(|p| p.name.clone()).collect());
                for p in params.iter_mut() {
                    if let Some(t) = &mut p.ty {
                        self.ty(t);
                    }
                }
                if let Some(rs) = ret {
                    for t in rs {
                        self.ty(t);
                    }
                }
                self.block(body);
                self.locals.pop();
            }
            ExprKind::Check(inner) => self.expr(inner),
            ExprKind::Conv { target, arg } => {
                self.ty(target);
                self.expr(arg);
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::NoneLit => {}
        }
    }
}
