//! nevla fmt: the canonical style printer
//! (docs/specs/2026-07-09-fmt-design.md). Parses the source, then prints
//! the AST back in the one true style, weaving the comments and blank
//! lines the trivia lexer preserved.

use crate::ast::*;
use crate::diag::Diag;
use crate::lexer::{self, Comment};
use crate::token::Token;

pub fn fmt_source(src: &str) -> Result<String, Diag> {
    let prog = crate::parser::parse(src)?;
    fmt_program(&prog, src)
}

/// Print an (possibly edited) AST in the one true style, weaving the
/// original source's comments and blank lines. `nevla tidy` edits the
/// import set and renders through here.
pub fn fmt_program(prog: &Program, src: &str) -> Result<String, Diag> {
    let (toks, trivia) = lexer::lex_trivia(src)?;
    let mut p = Printer {
        out: String::new(),
        indent: 0,
        comments: trivia.comments,
        ci: 0,
        blanks: trivia.blank_lines,
        last_line: 0,
        struct_fields: struct_field_lines(&toks),
    };
    p.program(&prog);
    while p.out.ends_with("\n\n") {
        p.out.pop();
    }
    if !p.out.ends_with('\n') {
        p.out.push('\n');
    }
    Ok(p.out)
}

/// Field-name lines per struct decl, recovered from the token stream (the
/// AST does not span individual fields). First identifier of each line
/// inside the braces is the field name.
fn struct_field_lines(toks: &[crate::token::Spanned<Token>]) -> Vec<(String, Vec<u32>)> {
    let mut out = vec![];
    let mut i = 0;
    while i < toks.len() {
        if toks[i].node == Token::Struct {
            if let Some(crate::token::Spanned {
                node: Token::Ident(name),
                ..
            }) = toks.get(i + 1)
            {
                let name = name.clone();
                let mut j = i + 2;
                let mut depth = 0;
                let mut lines = vec![];
                let mut cur_line = 0;
                while j < toks.len() {
                    match &toks[j].node {
                        Token::LBrace => depth += 1,
                        Token::RBrace => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Token::Ident(_) if depth == 1 && toks[j].line != cur_line => {
                            cur_line = toks[j].line;
                            lines.push(cur_line);
                        }
                        _ => {}
                    }
                    j += 1;
                }
                out.push((name, lines));
                i = j;
            }
        }
        i += 1;
    }
    out
}

struct Printer {
    out: String,
    indent: usize,
    comments: Vec<Comment>,
    ci: usize,
    blanks: Vec<u32>,
    last_line: u32,
    struct_fields: Vec<(String, Vec<u32>)>,
}

impl Printer {
    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
    }

    fn maybe_blank(&mut self, next_line: u32) {
        if self.last_line > 0
            && self
                .blanks
                .iter()
                .any(|&b| b > self.last_line && b < next_line)
        {
            self.out.push('\n');
        }
    }

    /// Own-line emission of every comment strictly before `line`.
    fn flush_comments_before(&mut self, line: u32, first: &mut bool) {
        while let Some(c) = self.comments.get(self.ci) {
            if c.line >= line {
                break;
            }
            let c = c.clone();
            self.ci += 1;
            if !*first {
                self.maybe_blank(c.line);
            }
            *first = false;
            self.write_indent();
            self.out.push_str("//");
            self.out.push_str(&c.text);
            self.out.push('\n');
            self.last_line = c.line;
        }
    }

    /// A trailing comment sitting on exactly this source line.
    fn attach_trailing(&mut self, line: u32) {
        if let Some(c) = self.comments.get(self.ci) {
            if c.line == line && !c.own_line {
                self.out.push_str("  //");
                self.out.push_str(&c.text);
                self.ci += 1;
            }
        }
    }

    // ---------- program ----------

    fn program(&mut self, prog: &Program) {
        let mut prev: Option<&Decl> = None;
        let mut i = 0;
        while i < prog.decls.len() {
            let d = &prog.decls[i];
            if matches!(d, Decl::Import { .. }) {
                // a run of imports is one group: a single line alone, the
                // factored block for two or more (the one true style)
                let mut j = i;
                while j < prog.decls.len() && matches!(prog.decls[j], Decl::Import { .. }) {
                    j += 1;
                }
                if prev.is_some() {
                    self.out.push('\n');
                }
                let mut first = prev.is_none();
                self.flush_comments_before(decl_line(d), &mut first);
                self.import_group(&prog.decls[i..j]);
                prev = Some(&prog.decls[j - 1]);
                i = j;
                continue;
            }
            let line = decl_line(d);
            if prev.is_some() {
                self.out.push('\n');
            }
            let mut first = prev.is_none();
            self.flush_comments_before(line, &mut first);
            self.decl(d);
            prev = Some(d);
            i += 1;
        }
        // comments after the last declaration
        let mut first = prev.is_none();
        self.flush_comments_before(u32::MAX, &mut first);
    }

    /// Emit one import group in source order: a single line alone, the
    /// factored block for two or more. fmt never reorders (its contract
    /// is AST preservation); sorting is `nevla tidy`'s job, the gofmt
    /// versus goimports split.
    fn import_group(&mut self, group: &[Decl]) {
        let spec = |d: &Decl| match d {
            Decl::Import { path, py, span, .. } => (path.clone(), *py, span.line),
            _ => unreachable!("import group holds only imports"),
        };
        if group.len() == 1 {
            let (path, py, line) = spec(&group[0]);
            self.out
                .push_str(if py { "import py " } else { "import " });
            self.push_str_lit(&path);
            self.attach_trailing(line);
            self.out.push('\n');
            self.last_line = line;
            return;
        }
        self.out.push_str("import (");
        self.out.push('\n');
        self.indent += 1;
        for d in group {
            let (path, py, line) = spec(d);
            let mut f = true;
            self.flush_comments_before(line, &mut f);
            self.write_indent();
            if py {
                self.out.push_str("py ");
            }
            self.push_str_lit(&path);
            self.attach_trailing(line);
            self.out.push('\n');
            self.last_line = line;
        }
        self.indent -= 1;
        self.out.push_str(")\n");
        self.last_line = spec(&group[group.len() - 1]).2;
    }

    fn decl(&mut self, d: &Decl) {
        match d {
            Decl::Import { path, py, span, .. } => {
                if *py {
                    self.out.push_str("import py ");
                } else {
                    self.out.push_str("import ");
                }
                self.push_str_lit(path);
                self.attach_trailing(span.line);
                self.out.push('\n');
                self.last_line = span.line;
            }
            Decl::Struct {
                name, fields, span, ..
            } => {
                self.out.push_str("struct ");
                self.out.push_str(name);
                self.out.push_str(" {");
                self.attach_trailing(span.line);
                self.out.push('\n');
                self.last_line = span.line;
                self.indent += 1;
                let lines = self
                    .struct_fields
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, l)| l.clone())
                    .unwrap_or_default();
                for (i, (f, t)) in fields.iter().enumerate() {
                    let fl = lines.get(i).copied().unwrap_or(0);
                    if fl > 0 {
                        let mut first = i == 0;
                        self.flush_comments_before(fl, &mut first);
                        if i > 0 {
                            self.maybe_blank(fl);
                        }
                    }
                    self.write_indent();
                    self.out.push_str(f);
                    self.out.push(' ');
                    self.type_expr(t);
                    if fl > 0 {
                        self.attach_trailing(fl);
                        self.last_line = fl;
                    }
                    self.out.push('\n');
                }
                self.indent -= 1;
                self.out.push_str("}\n");
            }
            Decl::Fn(f) => {
                self.out.push_str("fn ");
                self.out.push_str(&f.name);
                self.params(&f.params);
                self.ret_types(&f.ret);
                self.block(f.span.line, &f.body);
                self.out.push('\n');
                self.last_line = self.last_line.max(stmt_seq_max(&f.body));
            }
        }
    }

    fn params(&mut self, params: &[Param]) {
        self.out.push('(');
        for (i, p) in params.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.out.push_str(&p.name);
            if let Some(t) = &p.ty {
                self.out.push(' ');
                self.type_expr(t);
            }
        }
        self.out.push(')');
    }

    fn ret_types(&mut self, ret: &[TypeExpr]) {
        match ret {
            [] => {}
            [one] if !matches!(one, TypeExpr::Opt(_)) => {
                self.out.push(' ');
                self.type_expr(one);
            }
            many => {
                self.out.push_str(" (");
                for (i, t) in many.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.type_expr(t);
                }
                self.out.push(')');
            }
        }
    }

    fn type_expr(&mut self, t: &TypeExpr) {
        match t {
            TypeExpr::Named(n) => self.out.push_str(n),
            TypeExpr::List(e) => {
                self.out.push_str("[]");
                self.type_expr(e);
            }
            TypeExpr::Map(k, v) => {
                self.out.push_str("map[");
                self.type_expr(k);
                self.out.push(']');
                self.type_expr(v);
            }
            TypeExpr::Opt(e) => {
                self.type_expr(e);
                self.out.push('?');
            }
            TypeExpr::Fn(ps, rs) => {
                self.out.push_str("fn(");
                for (i, p) in ps.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.type_expr(p);
                }
                self.out.push(')');
                self.ret_types(rs);
            }
        }
    }

    // ---------- statements ----------

    /// `" {"` + header trailing + body + closing brace at current indent.
    fn block(&mut self, header_line: u32, b: &Block) {
        self.out.push_str(" {");
        self.attach_trailing(header_line);
        self.out.push('\n');
        self.last_line = self.last_line.max(header_line);
        self.indent += 1;
        self.stmt_seq(b);
        self.indent -= 1;
        self.write_indent();
        self.out.push('}');
    }

    fn stmt_seq(&mut self, b: &Block) {
        let mut first = true;
        for s in b {
            self.flush_comments_before(s.span.line, &mut first);
            if !first {
                self.maybe_blank(s.span.line);
            }
            first = false;
            self.write_indent();
            self.stmt(s);
            let anchor = stmt_max(s);
            self.attach_trailing(anchor);
            self.out.push('\n');
            self.last_line = self.last_line.max(anchor);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { names, expr } => {
                self.out.push_str(&names.join(", "));
                self.out.push_str(" := ");
                self.expr(expr);
            }
            StmtKind::Assign { target, expr } => {
                self.expr(target);
                self.out.push_str(" = ");
                self.expr(expr);
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Return(es) => {
                self.out.push_str("return");
                for (i, e) in es.iter().enumerate() {
                    self.out.push_str(if i == 0 { " " } else { ", " });
                    self.expr(e);
                }
            }
            StmtKind::If {
                cond,
                then,
                elifs,
                els,
            } => {
                self.out.push_str("if ");
                self.expr(cond);
                self.block(s.span.line, then);
                for (c, b) in elifs {
                    self.out.push_str(" else if ");
                    self.expr(c);
                    self.block(c.span.line, b);
                }
                if let Some(b) = els {
                    self.out.push_str(" else");
                    self.block(0, b);
                }
            }
            StmtKind::ForRange { names, iter, body } => {
                self.out.push_str("for ");
                if !names.is_empty() {
                    self.out.push_str(&names.join(", "));
                    self.out.push_str(" := ");
                }
                self.out.push_str("range ");
                self.expr(iter);
                self.block(s.span.line, body);
            }
            StmtKind::ForCond { cond, body } => {
                self.out.push_str("for");
                if let Some(c) = cond {
                    self.out.push(' ');
                    self.expr(c);
                }
                self.block(s.span.line, body);
            }
            StmtKind::With { expr, body } => {
                self.out.push_str("with ");
                self.expr(expr);
                self.block(s.span.line, body);
            }
            StmtKind::Break => self.out.push_str("break"),
            StmtKind::Continue => self.out.push_str("continue"),
        }
    }

    // ---------- expressions ----------

    fn expr(&mut self, e: &Expr) {
        self.expr_prec(e, 0);
    }

    /// Precedence-aware printing: parens reappear exactly where the parse
    /// requires them. Binary levels follow the parser (1..=6); unary and
    /// `check` sit at 7; postfix receivers demand 8.
    fn expr_prec(&mut self, e: &Expr, min: u8) {
        match &e.kind {
            ExprKind::Binary { op, lhs, rhs } => {
                let p = prec(*op);
                let parens = p < min;
                if parens {
                    self.out.push('(');
                }
                self.expr_prec(lhs, p);
                self.out.push(' ');
                self.out.push_str(op_str(*op));
                self.out.push(' ');
                self.expr_prec(rhs, p + 1);
                if parens {
                    self.out.push(')');
                }
            }
            ExprKind::Unary { op, rhs } => {
                let parens = 7 < min;
                if parens {
                    self.out.push('(');
                }
                self.out.push(match op {
                    UnOp::Not => '!',
                    UnOp::Neg => '-',
                });
                self.expr_prec(rhs, 7);
                if parens {
                    self.out.push(')');
                }
            }
            ExprKind::Check(inner) => {
                let parens = 7 < min;
                if parens {
                    self.out.push('(');
                }
                self.out.push_str("check ");
                self.expr_prec(inner, 7);
                if parens {
                    self.out.push(')');
                }
            }
            _ => self.postfix(e),
        }
    }

    fn postfix(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(i) => self.out.push_str(&i.to_string()),
            ExprKind::Float(f) => self.push_float(*f),
            ExprKind::Str(s) => self.push_str_lit(s),
            ExprKind::Bool(b) => self.out.push_str(if *b { "true" } else { "false" }),
            ExprKind::NoneLit => self.out.push_str("none"),
            ExprKind::Ident(n) => self.out.push_str(n),
            ExprKind::List(items) => {
                let refs: Vec<&Expr> = items.iter().collect();
                self.bracketed("[", "]", e.span.line, &refs, |p, x| p.expr(x));
            }
            ExprKind::ListLit { elem, items } => {
                self.out.push_str("[]");
                self.type_expr(elem);
                let refs: Vec<&Expr> = items.iter().collect();
                self.bracketed("{", "}", e.span.line, &refs, |p, x| p.expr(x));
            }
            ExprKind::MapLit { key, val, entries } => {
                self.out.push_str("map[");
                self.type_expr(key);
                self.out.push(']');
                self.type_expr(val);
                let refs: Vec<&(Expr, Expr)> = entries.iter().collect();
                self.bracketed("{", "}", e.span.line, &refs, |p, (k, v)| {
                    p.expr(k);
                    p.out.push_str(": ");
                    p.expr(v);
                });
            }
            ExprKind::StructLit { name, fields } => {
                self.out.push_str(name);
                let refs: Vec<&(String, Expr)> = fields.iter().collect();
                self.bracketed("{", "}", e.span.line, &refs, |p, (n, v)| {
                    p.out.push_str(n);
                    p.out.push_str(": ");
                    p.expr(v);
                });
            }
            ExprKind::Call {
                callee,
                args,
                kwargs,
            } => {
                self.expr_prec(callee, 8);
                self.call_args(e.span.line, args, kwargs);
            }
            ExprKind::Method {
                recv,
                name,
                args,
                kwargs,
            } => {
                self.expr_prec(recv, 8);
                self.out.push('.');
                self.out.push_str(name);
                self.call_args(e.span.line, args, kwargs);
            }
            ExprKind::Field { recv, name } => {
                self.expr_prec(recv, 8);
                self.out.push('.');
                self.out.push_str(name);
            }
            ExprKind::Index { recv, idx } => {
                self.expr_prec(recv, 8);
                self.out.push('[');
                self.expr(idx);
                self.out.push(']');
            }
            ExprKind::Slice { recv, lo, hi } => {
                self.expr_prec(recv, 8);
                self.out.push('[');
                self.expr(lo);
                self.out.push(':');
                self.expr(hi);
                self.out.push(']');
            }
            ExprKind::Conv { target, arg } => {
                self.type_expr(target);
                self.out.push('(');
                self.expr(arg);
                self.out.push(')');
            }
            ExprKind::Lambda { params, ret, body } => {
                self.out.push_str("fn");
                self.params(params);
                if let Some(r) = ret {
                    self.ret_types(r);
                }
                let inline = body.len() == 1
                    && matches!(body[0].kind, StmtKind::Expr(_) | StmtKind::Return(_));
                if inline {
                    self.out.push_str(" { ");
                    self.stmt(&body[0]);
                    self.out.push_str(" }");
                } else {
                    self.block(e.span.line, body);
                }
            }
            _ => unreachable!("binary/unary/check handled in expr_prec"),
        }
    }

    /// A delimited, comma-separated list that keeps the source's break
    /// decision: any element starting past the opener's line goes one
    /// element per line with a trailing comma.
    fn bracketed<T>(
        &mut self,
        open: &str,
        close: &str,
        open_line: u32,
        items: &[&T],
        mut item: impl FnMut(&mut Self, &T),
    ) where
        T: HasLine,
    {
        let multiline = items.iter().any(|x| x.line() > open_line);
        self.out.push_str(open);
        if multiline {
            self.out.push('\n');
            self.indent += 1;
            for x in items {
                self.write_indent();
                item(self, x);
                self.out.push_str(",\n");
            }
            self.indent -= 1;
            self.write_indent();
        } else {
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    self.out.push_str(", ");
                }
                item(self, x);
            }
        }
        self.out.push_str(close);
    }

    fn call_args(&mut self, open_line: u32, args: &[Expr], kwargs: &[(String, Expr)]) {
        enum Arg<'a> {
            Pos(&'a Expr),
            Kw(&'a String, &'a Expr),
        }
        impl HasLine for Arg<'_> {
            fn line(&self) -> u32 {
                match self {
                    Arg::Pos(e) => e.span.line,
                    Arg::Kw(_, e) => e.span.line,
                }
            }
        }
        let all: Vec<Arg> = args
            .iter()
            .map(Arg::Pos)
            .chain(kwargs.iter().map(|(n, e)| Arg::Kw(n, e)))
            .collect();
        let refs: Vec<&Arg> = all.iter().collect();
        self.bracketed("(", ")", open_line, &refs, |p, a| match a {
            Arg::Pos(e) => p.expr(e),
            Arg::Kw(n, e) => {
                p.out.push_str(n);
                p.out.push_str(": ");
                p.expr(e);
            }
        });
    }

    fn push_str_lit(&mut self, s: &str) {
        self.out.push('"');
        for c in s.chars() {
            match c {
                '\\' => self.out.push_str("\\\\"),
                '"' => self.out.push_str("\\\""),
                '\n' => self.out.push_str("\\n"),
                '\t' => self.out.push_str("\\t"),
                c => self.out.push(c),
            }
        }
        self.out.push('"');
    }

    fn push_float(&mut self, f: f64) {
        let s = format!("{f:?}");
        if s.contains(['e', 'E']) {
            // the lexer has no exponent syntax; expand and trim
            let mut d = format!("{f:.324}");
            while d.ends_with('0') && !d.ends_with(".0") {
                d.pop();
            }
            self.out.push_str(&d);
        } else {
            self.out.push_str(&s);
        }
    }
}

trait HasLine {
    fn line(&self) -> u32;
}
impl HasLine for Expr {
    fn line(&self) -> u32 {
        self.span.line
    }
}
impl HasLine for (Expr, Expr) {
    fn line(&self) -> u32 {
        self.0.span.line
    }
}
impl HasLine for (String, Expr) {
    fn line(&self) -> u32 {
        self.1.span.line
    }
}

fn prec(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::Eq | BinOp::NotEq => 3,
        BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => 4,
        BinOp::Add | BinOp::Sub => 5,
        BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::MatMul => 6,
    }
}

fn op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::MatMul => "@",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::LtEq => "<=",
        BinOp::Gt => ">",
        BinOp::GtEq => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
    }
}

fn decl_line(d: &Decl) -> u32 {
    match d {
        Decl::Fn(f) => f.span.line,
        Decl::Struct { span, .. } | Decl::Import { span, .. } => span.line,
    }
}

/// Deepest source line a statement reaches, for trailing-comment anchors
/// and blank-line bookkeeping.
fn stmt_max(s: &Stmt) -> u32 {
    let mut m = s.span.line;
    match &s.kind {
        StmtKind::Let { expr, .. } | StmtKind::Expr(expr) | StmtKind::With { expr, .. } => {
            m = m.max(expr_max(expr));
        }
        StmtKind::Assign { target, expr } => {
            m = m.max(expr_max(target)).max(expr_max(expr));
        }
        StmtKind::Return(es) => {
            for e in es {
                m = m.max(expr_max(e));
            }
        }
        StmtKind::If {
            cond,
            then,
            elifs,
            els,
        } => {
            m = m.max(expr_max(cond)).max(stmt_seq_max(then));
            for (c, b) in elifs {
                m = m.max(expr_max(c)).max(stmt_seq_max(b));
            }
            if let Some(b) = els {
                m = m.max(stmt_seq_max(b));
            }
        }
        StmtKind::ForRange { iter, body, .. } => {
            m = m.max(expr_max(iter)).max(stmt_seq_max(body));
        }
        StmtKind::ForCond { cond, body } => {
            if let Some(c) = cond {
                m = m.max(expr_max(c));
            }
            m = m.max(stmt_seq_max(body));
        }
        StmtKind::Break | StmtKind::Continue => {}
    }
    if let StmtKind::With { body, .. } = &s.kind {
        m = m.max(stmt_seq_max(body));
    }
    m
}

fn stmt_seq_max(b: &Block) -> u32 {
    b.iter().map(stmt_max).max().unwrap_or(0)
}

fn expr_max(e: &Expr) -> u32 {
    let mut m = e.span.line;
    let mut kids: Vec<&Expr> = vec![];
    match &e.kind {
        ExprKind::List(items) => kids.extend(items),
        ExprKind::ListLit { items, .. } => kids.extend(items),
        ExprKind::MapLit { entries, .. } => {
            for (k, v) in entries {
                kids.push(k);
                kids.push(v);
            }
        }
        ExprKind::StructLit { fields, .. } => kids.extend(fields.iter().map(|(_, v)| v)),
        ExprKind::Unary { rhs, .. } => kids.push(rhs),
        ExprKind::Binary { lhs, rhs, .. } => {
            kids.push(lhs);
            kids.push(rhs);
        }
        ExprKind::Call {
            callee,
            args,
            kwargs,
        } => {
            kids.push(callee);
            kids.extend(args);
            kids.extend(kwargs.iter().map(|(_, v)| v));
        }
        ExprKind::Method {
            recv, args, kwargs, ..
        } => {
            kids.push(recv);
            kids.extend(args);
            kids.extend(kwargs.iter().map(|(_, v)| v));
        }
        ExprKind::Field { recv, .. } => kids.push(recv),
        ExprKind::Index { recv, idx } => {
            kids.push(recv);
            kids.push(idx);
        }
        ExprKind::Slice { recv, lo, hi } => {
            kids.push(recv);
            kids.push(lo);
            kids.push(hi);
        }
        ExprKind::Check(inner) | ExprKind::Conv { arg: inner, .. } => kids.push(inner),
        ExprKind::Lambda { body, .. } => {
            m = m.max(stmt_seq_max(body));
        }
        _ => {}
    }
    for k in kids {
        m = m.max(expr_max(k));
    }
    m
}
