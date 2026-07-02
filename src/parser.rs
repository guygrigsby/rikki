use crate::ast::*;
use crate::diag::Diag;
use crate::lexer::lex;
use crate::token::{Spanned, Token};

pub fn parse(src: &str) -> Result<Program, Diag> {
    let toks = lex(src)?;
    Parser { toks, pos: 0, depth: 0 }.program()
}

/// Parse a single expression (repl).
pub fn parse_expr(src: &str) -> Result<Expr, Diag> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let e = p.expr(true)?;
    p.skip_nl();
    if !p.at_end() {
        return Err(p.err_here("unexpected trailing tokens"));
    }
    Ok(e)
}

struct Parser {
    toks: Vec<Spanned<Token>>,
    pos: usize,
    /// Current recursive-descent nesting depth (expressions, blocks, types).
    depth: usize,
}

const CONV_NAMES: &[&str] = &["int", "float", "str", "bool"];

/// Cap on recursive-descent nesting. Chosen empirically: at this limit a
/// debug build peaks under 4 MiB of stack (worst case, paren nesting), well
/// inside the 8 MiB main-thread stack the CLI parses on. Note plain 2 MiB
/// worker threads fit only ~160; parse on the main thread or a bigger stack.
const MAX_DEPTH: usize = 256;

impl Parser {
    fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos).map(|s| &s.node)
    }

    fn peek2(&self) -> Option<&Token> {
        self.toks.get(self.pos + 1).map(|s| &s.node)
    }

    fn here(&self) -> (u32, u32) {
        self.toks
            .get(self.pos.min(self.toks.len().saturating_sub(1)))
            .map(|s| (s.line, s.col))
            .unwrap_or((1, 1))
    }

    fn err_here(&self, msg: &str) -> Diag {
        let (line, col) = self.here();
        Diag { msg: msg.into(), line, col }
    }

    fn enter(&mut self) -> Result<(), Diag> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(self.err_here("expression too deeply nested"))
        } else {
            Ok(())
        }
    }

    fn exit(&mut self) {
        self.depth -= 1;
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.toks.get(self.pos).map(|s| s.node.clone());
        self.pos += 1;
        t
    }

    fn eat(&mut self, t: &Token) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Token, what: &str) -> Result<(), Diag> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(self.err_here(&format!("expected {what}")))
        }
    }

    fn skip_nl(&mut self) {
        while self.peek() == Some(&Token::Newline) {
            self.pos += 1;
        }
    }

    fn ident(&mut self, what: &str) -> Result<String, Diag> {
        match self.peek() {
            Some(Token::Ident(_)) => match self.bump() {
                Some(Token::Ident(s)) => Ok(s),
                _ => unreachable!(),
            },
            _ => Err(self.err_here(&format!("expected {what}"))),
        }
    }

    // ---------- program ----------

    fn program(&mut self) -> Result<Program, Diag> {
        let mut decls = vec![];
        self.skip_nl();
        while !self.at_end() {
            decls.push(self.decl()?);
            self.skip_nl();
        }
        Ok(Program { decls })
    }

    fn decl(&mut self) -> Result<Decl, Diag> {
        let (line, col) = self.here();
        match self.peek() {
            Some(Token::Import) => {
                self.bump();
                let py = self.eat(&Token::Py);
                match self.bump() {
                    Some(Token::Str(path)) => Ok(Decl::Import { path, py, line, col }),
                    _ => Err(self.err_here("expected import path string")),
                }
            }
            Some(Token::Struct) => {
                self.bump();
                let name = self.ident("struct name")?;
                self.expect(&Token::LBrace, "{")?;
                self.skip_nl();
                let mut fields = vec![];
                while self.peek() != Some(&Token::RBrace) {
                    let f = self.ident("field name")?;
                    self.expect(&Token::Colon, ":")?;
                    let ty = self.type_expr()?;
                    fields.push((f, ty));
                    if !self.eat(&Token::Comma) && self.peek() != Some(&Token::Newline) {
                        break;
                    }
                    self.skip_nl();
                }
                self.expect(&Token::RBrace, "}")?;
                Ok(Decl::Struct { name, fields, line, col })
            }
            Some(Token::Fn) => {
                self.bump();
                let name = self.ident("function name")?;
                let params = self.params()?;
                let ret = self.ret_types()?;
                let body = self.block()?;
                Ok(Decl::Fn(FnDecl { name, params, ret, body, line, col }))
            }
            _ => Err(self.err_here("expected fn, struct, or import")),
        }
    }

    fn params(&mut self) -> Result<Vec<Param>, Diag> {
        self.expect(&Token::LParen, "(")?;
        self.skip_nl();
        let mut out = vec![];
        while self.peek() != Some(&Token::RParen) {
            let name = self.ident("parameter name")?;
            let ty = if self.eat(&Token::Colon) { Some(self.type_expr()?) } else { None };
            out.push(Param { name, ty });
            if !self.eat(&Token::Comma) {
                break;
            }
            self.skip_nl();
        }
        self.skip_nl();
        self.expect(&Token::RParen, ")")?;
        Ok(out)
    }

    fn ret_types(&mut self) -> Result<Vec<TypeExpr>, Diag> {
        match self.peek() {
            Some(Token::LBrace) | None => Ok(vec![]),
            Some(Token::LParen) => {
                self.bump();
                self.skip_nl();
                let mut out = vec![];
                while self.peek() != Some(&Token::RParen) {
                    out.push(self.type_expr()?);
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                    self.skip_nl();
                }
                self.skip_nl();
                self.expect(&Token::RParen, ")")?;
                Ok(out)
            }
            _ => Ok(vec![self.type_expr()?]),
        }
    }

    fn maybe_opt(&mut self, base: TypeExpr) -> TypeExpr {
        if self.eat(&Token::Question) {
            TypeExpr::Opt(Box::new(base))
        } else {
            base
        }
    }

    fn type_expr(&mut self) -> Result<TypeExpr, Diag> {
        self.enter()?;
        let r = self.type_expr_inner();
        self.exit();
        r
    }

    fn type_expr_inner(&mut self) -> Result<TypeExpr, Diag> {
        let base = match self.peek().cloned() {
            Some(Token::Py) => {
                self.bump();
                TypeExpr::Named("py".into())
            }
            Some(Token::Fn) => {
                self.bump();
                self.expect(&Token::LParen, "(")?;
                let mut args = vec![];
                while self.peek() != Some(&Token::RParen) {
                    args.push(self.type_expr()?);
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                }
                self.expect(&Token::RParen, ")")?;
                // returns: absent, single type, or parenthesized list
                let rets = match self.peek() {
                    Some(Token::LParen) => {
                        self.bump();
                        let mut r = vec![];
                        while self.peek() != Some(&Token::RParen) {
                            r.push(self.type_expr()?);
                            if !self.eat(&Token::Comma) {
                                break;
                            }
                        }
                        self.expect(&Token::RParen, ")")?;
                        r
                    }
                    Some(Token::Ident(_)) | Some(Token::Py) => vec![self.type_expr()?],
                    _ => vec![],
                };
                TypeExpr::Fn(args, rets)
            }
            Some(Token::Ident(name)) => {
                self.bump();
                match name.as_str() {
                    "list" => {
                        self.expect(&Token::LBracket, "[")?;
                        let inner = self.type_expr()?;
                        self.expect(&Token::RBracket, "]")?;
                        TypeExpr::List(Box::new(inner))
                    }
                    "map" => {
                        self.expect(&Token::LBracket, "[")?;
                        let k = self.type_expr()?;
                        self.expect(&Token::Comma, ",")?;
                        let v = self.type_expr()?;
                        self.expect(&Token::RBracket, "]")?;
                        TypeExpr::Map(Box::new(k), Box::new(v))
                    }
                    _ => {
                        if self.peek() == Some(&Token::Dot) {
                            if let Some(Token::Ident(_)) = self.peek2() {
                                self.bump();
                                let member = self.ident("type name")?;
                                return Ok(self.maybe_opt(TypeExpr::Named(format!("{name}.{member}"))));
                            }
                        }
                        TypeExpr::Named(name)
                    }
                }
            }
            _ => return Err(self.err_here("expected type")),
        };
        if self.eat(&Token::Question) {
            Ok(TypeExpr::Opt(Box::new(base)))
        } else {
            Ok(base)
        }
    }

    // ---------- statements ----------

    fn block(&mut self) -> Result<Block, Diag> {
        self.enter()?;
        let r = self.block_inner();
        self.exit();
        r
    }

    fn block_inner(&mut self) -> Result<Block, Diag> {
        self.expect(&Token::LBrace, "{")?;
        self.skip_nl();
        let mut out = vec![];
        while self.peek() != Some(&Token::RBrace) {
            if self.at_end() {
                return Err(self.err_here("unclosed block"));
            }
            out.push(self.stmt()?);
            // separator: newline(s) or the closing brace
            if self.peek() != Some(&Token::RBrace) {
                self.expect(&Token::Newline, "newline")?;
                self.skip_nl();
            }
        }
        self.expect(&Token::RBrace, "}")?;
        Ok(out)
    }

    fn stmt(&mut self) -> Result<Stmt, Diag> {
        let (line, col) = self.here();
        let kind = match self.peek() {
            Some(Token::Return) => {
                self.bump();
                let mut exprs = vec![];
                if !matches!(self.peek(), Some(Token::Newline) | Some(Token::RBrace) | None) {
                    exprs.push(self.expr(true)?);
                    while self.eat(&Token::Comma) {
                        self.skip_nl();
                        exprs.push(self.expr(true)?);
                    }
                }
                StmtKind::Return(exprs)
            }
            Some(Token::Break) => {
                self.bump();
                StmtKind::Break
            }
            Some(Token::Continue) => {
                self.bump();
                StmtKind::Continue
            }
            Some(Token::If) => self.if_stmt()?,
            Some(Token::For) => self.for_stmt()?,
            _ => {
                // try: ident list := expr
                let save = self.pos;
                if let Some(names) = self.try_name_list(&Token::ColonEq) {
                    let expr = self.expr(true)?;
                    StmtKind::Let { names, expr }
                } else {
                    self.pos = save;
                    let e = self.expr(true)?;
                    if self.eat(&Token::Eq) {
                        match e.kind {
                            ExprKind::Ident(_) | ExprKind::Index { .. } | ExprKind::Field { .. } => {
                                let rhs = self.expr(true)?;
                                StmtKind::Assign { target: e, expr: rhs }
                            }
                            _ => return Err(self.err_here("cannot assign to this expression")),
                        }
                    } else {
                        StmtKind::Expr(e)
                    }
                }
            }
        };
        Ok(Stmt { kind, line, col })
    }

    /// If the upcoming tokens are `ident (, ident)* <marker>`, consume through
    /// the marker and return the names. Otherwise leave position unchanged.
    fn try_name_list(&mut self, marker: &Token) -> Option<Vec<String>> {
        let save = self.pos;
        let mut names = vec![];
        loop {
            match self.peek() {
                Some(Token::Ident(s)) => {
                    names.push(s.clone());
                    self.pos += 1;
                }
                _ => {
                    self.pos = save;
                    return None;
                }
            }
            if self.peek() == Some(marker) {
                self.pos += 1;
                return Some(names);
            }
            if !self.eat(&Token::Comma) {
                self.pos = save;
                return None;
            }
        }
    }

    fn if_stmt(&mut self) -> Result<StmtKind, Diag> {
        self.expect(&Token::If, "if")?;
        let cond = self.expr(false)?;
        let then = self.block()?;
        let mut elifs = vec![];
        let mut els = None;
        while self.eat(&Token::Else) {
            if self.eat(&Token::If) {
                let c = self.expr(false)?;
                let b = self.block()?;
                elifs.push((c, b));
            } else {
                els = Some(self.block()?);
                break;
            }
        }
        Ok(StmtKind::If { cond, then, elifs, els })
    }

    fn for_stmt(&mut self) -> Result<StmtKind, Diag> {
        self.expect(&Token::For, "for")?;
        if self.peek() == Some(&Token::LBrace) {
            let body = self.block()?;
            return Ok(StmtKind::ForCond { cond: None, body });
        }
        if let Some(names) = self.try_name_list(&Token::In) {
            let iter = self.expr(false)?;
            let body = self.block()?;
            return Ok(StmtKind::ForIn { names, iter, body });
        }
        let cond = self.expr(false)?;
        let body = self.block()?;
        Ok(StmtKind::ForCond { cond: Some(cond), body })
    }

    // ---------- expressions ----------

    /// `struct_ok` is false in if/for headers where `Name{` would swallow the block.
    fn expr(&mut self, struct_ok: bool) -> Result<Expr, Diag> {
        self.enter()?;
        let r = self.binary(0, struct_ok);
        self.exit();
        r
    }

    fn binary(&mut self, min_prec: u8, struct_ok: bool) -> Result<Expr, Diag> {
        let mut lhs = self.unary(struct_ok)?;
        loop {
            let (op, prec) = match self.peek() {
                Some(Token::OrOr) => (BinOp::Or, 1),
                Some(Token::AndAnd) => (BinOp::And, 2),
                Some(Token::EqEq) => (BinOp::Eq, 3),
                Some(Token::NotEq) => (BinOp::NotEq, 3),
                Some(Token::Lt) => (BinOp::Lt, 4),
                Some(Token::LtEq) => (BinOp::LtEq, 4),
                Some(Token::Gt) => (BinOp::Gt, 4),
                Some(Token::GtEq) => (BinOp::GtEq, 4),
                Some(Token::Plus) => (BinOp::Add, 5),
                Some(Token::Minus) => (BinOp::Sub, 5),
                Some(Token::Star) => (BinOp::Mul, 6),
                Some(Token::Slash) => (BinOp::Div, 6),
                Some(Token::Percent) => (BinOp::Rem, 6),
                _ => break,
            };
            if prec < min_prec {
                break;
            }
            let (line, col) = self.here();
            self.bump();
            self.skip_nl();
            let rhs = self.binary(prec + 1, struct_ok)?;
            lhs = Expr {
                kind: ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) },
                line,
                col,
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self, struct_ok: bool) -> Result<Expr, Diag> {
        self.enter()?;
        let r = self.unary_inner(struct_ok);
        self.exit();
        r
    }

    fn unary_inner(&mut self, struct_ok: bool) -> Result<Expr, Diag> {
        let (line, col) = self.here();
        match self.peek() {
            Some(Token::Check) => {
                self.bump();
                let rhs = self.unary(struct_ok)?;
                Ok(Expr { kind: ExprKind::Check(Box::new(rhs)), line, col })
            }
            Some(Token::Bang) => {
                self.bump();
                let rhs = self.unary(struct_ok)?;
                Ok(Expr { kind: ExprKind::Unary { op: UnOp::Not, rhs: Box::new(rhs) }, line, col })
            }
            Some(Token::Minus) => {
                self.bump();
                let rhs = self.unary(struct_ok)?;
                Ok(Expr { kind: ExprKind::Unary { op: UnOp::Neg, rhs: Box::new(rhs) }, line, col })
            }
            _ => self.postfix(struct_ok),
        }
    }

    fn postfix(&mut self, struct_ok: bool) -> Result<Expr, Diag> {
        let mut e = self.primary(struct_ok)?;
        loop {
            let (line, col) = self.here();
            match self.peek() {
                Some(Token::Dot) => {
                    self.bump();
                    let name = self.ident("member name")?;
                    if self.peek() == Some(&Token::LParen) {
                        let args = self.call_args()?;
                        e = Expr {
                            kind: ExprKind::Method { recv: Box::new(e), name, args },
                            line,
                            col,
                        };
                    } else {
                        e = Expr { kind: ExprKind::Field { recv: Box::new(e), name }, line, col };
                    }
                }
                Some(Token::LParen) => {
                    let args = self.call_args()?;
                    e = Expr { kind: ExprKind::Call { callee: Box::new(e), args }, line, col };
                }
                Some(Token::LBracket) => {
                    self.bump();
                    self.skip_nl();
                    let first = self.expr(true)?;
                    if self.eat(&Token::Colon) {
                        let hi = self.expr(true)?;
                        self.expect(&Token::RBracket, "]")?;
                        e = Expr {
                            kind: ExprKind::Slice {
                                recv: Box::new(e),
                                lo: Box::new(first),
                                hi: Box::new(hi),
                            },
                            line,
                            col,
                        };
                    } else {
                        self.skip_nl();
                        self.expect(&Token::RBracket, "]")?;
                        e = Expr {
                            kind: ExprKind::Index { recv: Box::new(e), idx: Box::new(first) },
                            line,
                            col,
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn call_args(&mut self) -> Result<Vec<Expr>, Diag> {
        self.expect(&Token::LParen, "(")?;
        self.skip_nl();
        let mut args = vec![];
        while self.peek() != Some(&Token::RParen) {
            args.push(self.expr(true)?);
            self.skip_nl();
            if !self.eat(&Token::Comma) {
                break;
            }
            self.skip_nl();
        }
        self.expect(&Token::RParen, ")")?;
        Ok(args)
    }

    fn primary(&mut self, struct_ok: bool) -> Result<Expr, Diag> {
        let (line, col) = self.here();
        let mk = |kind| Expr { kind, line, col };
        match self.peek().cloned() {
            Some(Token::Int(v)) => {
                self.bump();
                Ok(mk(ExprKind::Int(v)))
            }
            Some(Token::Float(v)) => {
                self.bump();
                Ok(mk(ExprKind::Float(v)))
            }
            Some(Token::Str(s)) => {
                self.bump();
                Ok(mk(ExprKind::Str(s)))
            }
            Some(Token::True) => {
                self.bump();
                Ok(mk(ExprKind::Bool(true)))
            }
            Some(Token::False) => {
                self.bump();
                Ok(mk(ExprKind::Bool(false)))
            }
            Some(Token::None_) => {
                self.bump();
                Ok(mk(ExprKind::NoneLit))
            }
            Some(Token::LParen) => {
                self.bump();
                self.skip_nl();
                let e = self.expr(true)?;
                self.skip_nl();
                self.expect(&Token::RParen, ")")?;
                Ok(e)
            }
            Some(Token::LBracket) => {
                self.bump();
                self.skip_nl();
                let mut items = vec![];
                while self.peek() != Some(&Token::RBracket) {
                    items.push(self.expr(true)?);
                    self.skip_nl();
                    if !self.eat(&Token::Comma) {
                        break;
                    }
                    self.skip_nl();
                }
                self.expect(&Token::RBracket, "]")?;
                Ok(mk(ExprKind::List(items)))
            }
            Some(Token::Fn) => {
                self.bump();
                let params = self.params()?;
                let ret = match self.peek() {
                    Some(Token::LBrace) => None,
                    _ => Some(self.ret_types()?),
                };
                let body = self.block()?;
                Ok(mk(ExprKind::Lambda { params, ret, body }))
            }
            Some(Token::Ident(name)) => {
                // conversions: int(x), str(x), float(x), bool(x), list[T](x)
                if CONV_NAMES.contains(&name.as_str()) && self.peek2() == Some(&Token::LParen) {
                    self.bump();
                    let target = TypeExpr::Named(name);
                    let mut args = self.call_args()?;
                    if args.len() != 1 {
                        return Err(self.err_here("conversion takes one argument"));
                    }
                    return Ok(mk(ExprKind::Conv { target, arg: Box::new(args.remove(0)) }));
                }
                if name == "list" && self.peek2() == Some(&Token::LBracket) {
                    let target = self.type_expr()?;
                    let mut args = self.call_args()?;
                    if args.len() != 1 {
                        return Err(self.err_here("conversion takes one argument"));
                    }
                    return Ok(mk(ExprKind::Conv { target, arg: Box::new(args.remove(0)) }));
                }
                if name == "map" && self.peek2() == Some(&Token::LBracket) {
                    let ty = self.type_expr()?;
                    let (key, val) = match ty {
                        TypeExpr::Map(k, v) => (*k, *v),
                        _ => return Err(self.err_here("expected map type")),
                    };
                    self.expect(&Token::LBrace, "{")?;
                    self.skip_nl();
                    let mut entries = vec![];
                    while self.peek() != Some(&Token::RBrace) {
                        let k = self.expr(true)?;
                        self.expect(&Token::Colon, ":")?;
                        let v = self.expr(true)?;
                        entries.push((k, v));
                        self.skip_nl();
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                        self.skip_nl();
                    }
                    self.expect(&Token::RBrace, "}")?;
                    return Ok(mk(ExprKind::MapLit { key, val, entries }));
                }
                self.bump();
                if struct_ok && self.peek() == Some(&Token::LBrace) {
                    self.bump();
                    self.skip_nl();
                    let mut fields = vec![];
                    while self.peek() != Some(&Token::RBrace) {
                        let f = self.ident("field name")?;
                        self.expect(&Token::Colon, ":")?;
                        let v = self.expr(true)?;
                        fields.push((f, v));
                        self.skip_nl();
                        if !self.eat(&Token::Comma) {
                            break;
                        }
                        self.skip_nl();
                    }
                    self.expect(&Token::RBrace, "}")?;
                    return Ok(mk(ExprKind::StructLit { name, fields }));
                }
                Ok(mk(ExprKind::Ident(name)))
            }
            _ => Err(self.err_here("expected expression")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ExprKind as E;

    fn expr(src: &str) -> Expr {
        parse_expr(src).unwrap()
    }

    fn one_fn(src: &str) -> FnDecl {
        let p = parse(src).unwrap();
        match p.decls.into_iter().next().unwrap() {
            Decl::Fn(f) => f,
            d => panic!("expected fn, got {d:?}"),
        }
    }

    #[test]
    fn precedence() {
        // 1 + 2 * 3 → 1 + (2 * 3)
        let e = expr("1 + 2 * 3");
        match e.kind {
            E::Binary { op: BinOp::Add, rhs, .. } => {
                assert!(matches!(rhs.kind, E::Binary { op: BinOp::Mul, .. }));
            }
            k => panic!("{k:?}"),
        }
        // !a && b || c → ((!a) && b) || c
        let e = expr("!a && b || c");
        assert!(matches!(e.kind, E::Binary { op: BinOp::Or, .. }));
    }

    #[test]
    fn method_chain() {
        let e = expr("xs.map(f).filter(g)");
        match e.kind {
            E::Method { recv, name, .. } => {
                assert_eq!(name, "filter");
                assert!(matches!(recv.kind, E::Method { .. }));
            }
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn check_binds_whole_chain() {
        let e = expr("check torch.randn([2, 3])");
        match e.kind {
            E::Check(inner) => assert!(matches!(inner.kind, E::Method { .. })),
            k => panic!("{k:?}"),
        }
        // check f() + 1 → (check f()) + 1
        let e = expr("check f() + 1");
        assert!(matches!(e.kind, E::Binary { op: BinOp::Add, .. }));
    }

    #[test]
    fn lambdas() {
        let e = expr("fn(x) { x > 2 }");
        match e.kind {
            E::Lambda { params, ret, body } => {
                assert_eq!(params.len(), 1);
                assert!(params[0].ty.is_none());
                assert!(ret.is_none());
                assert_eq!(body.len(), 1);
            }
            k => panic!("{k:?}"),
        }
        let e = expr("fn(x: int) int { return x * 2 }");
        match e.kind {
            E::Lambda { params, ret, .. } => {
                assert_eq!(params[0].ty, Some(TypeExpr::Named("int".into())));
                assert_eq!(ret, Some(vec![TypeExpr::Named("int".into())]));
            }
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn conversions() {
        let e = expr("list[int](t.shape)");
        match e.kind {
            E::Conv { target, .. } => {
                assert_eq!(target, TypeExpr::List(Box::new(TypeExpr::Named("int".into()))));
            }
            k => panic!("{k:?}"),
        }
        assert!(matches!(expr("int(x)").kind, E::Conv { .. }));
    }

    #[test]
    fn fn_decl_multi_return() {
        let f = one_fn("fn fetch(url: str) (str, error?) {\n    return \"\", none\n}\n");
        assert_eq!(f.name, "fetch");
        assert_eq!(
            f.ret,
            vec![
                TypeExpr::Named("str".into()),
                TypeExpr::Opt(Box::new(TypeExpr::Named("error".into())))
            ]
        );
        assert!(matches!(f.body[0].kind, StmtKind::Return(ref es) if es.len() == 2));
    }

    #[test]
    fn let_destructure() {
        let f = one_fn("fn main() {\n    a, b := f()\n}\n");
        assert!(matches!(
            f.body[0].kind,
            StmtKind::Let { ref names, .. } if names == &vec!["a".to_string(), "b".to_string()]
        ));
    }

    #[test]
    fn if_else_chain() {
        let f = one_fn("fn main() {\n    if a {\n        x()\n    } else if b {\n        y()\n    } else {\n        z()\n    }\n}\n");
        match &f.body[0].kind {
            StmtKind::If { elifs, els, .. } => {
                assert_eq!(elifs.len(), 1);
                assert!(els.is_some());
            }
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn for_forms() {
        let src = "fn main() {\n    for x in xs {\n        p(x)\n    }\n    for k, v in m {\n        p(k)\n    }\n    for a < 3 {\n        q()\n    }\n    for {\n        break\n    }\n}\n";
        let f = one_fn(src);
        assert!(matches!(f.body[0].kind, StmtKind::ForIn { ref names, .. } if names.len() == 1));
        assert!(matches!(f.body[1].kind, StmtKind::ForIn { ref names, .. } if names.len() == 2));
        assert!(matches!(f.body[2].kind, StmtKind::ForCond { cond: Some(_), .. }));
        assert!(matches!(f.body[3].kind, StmtKind::ForCond { cond: None, .. }));
    }

    #[test]
    fn imports() {
        let p = parse("import \"http\"\nimport py \"torch\"\n").unwrap();
        assert_eq!(
            p.decls,
            vec![
                Decl::Import { path: "http".into(), py: false, line: 1, col: 1 },
                Decl::Import { path: "torch".into(), py: true, line: 2, col: 1 },
            ]
        );
    }

    #[test]
    fn struct_decl_and_literal() {
        let src = "struct User {\n    name: str\n    age: int\n}\nfn main() {\n    u := User{name: \"guy\", age: 44}\n    print(u.name)\n}\n";
        let p = parse(src).unwrap();
        assert!(matches!(&p.decls[0], Decl::Struct { fields, .. } if fields.len() == 2));
    }

    #[test]
    fn struct_lit_blocked_in_headers() {
        // `if u != none {` must not parse `none {` weirdness; and `if User{...}` is disallowed
        let f = one_fn("fn main() {\n    if u != none {\n        p(u)\n    }\n}\n");
        assert!(matches!(f.body[0].kind, StmtKind::If { .. }));
    }

    #[test]
    fn map_literal() {
        let e = expr("map[str, int]{\"a\": 1, \"b\": 2}");
        match e.kind {
            E::MapLit { entries, .. } => assert_eq!(entries.len(), 2),
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn index_and_slice() {
        assert!(matches!(expr("xs[1]").kind, E::Index { .. }));
        assert!(matches!(expr("xs[1:3]").kind, E::Slice { .. }));
    }

    /// Run `f` on a thread with the 8 MiB stack the CLI's main thread gets;
    /// libtest worker threads only get 2 MiB, which MAX_DEPTH does not target.
    fn on_main_sized_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(8 << 20)
            .spawn(f)
            .unwrap()
            .join()
            .unwrap()
    }

    #[test]
    fn deep_paren_nesting_errors_not_crashes() {
        let err = on_main_sized_stack(|| {
            let n = 50_000;
            let src = format!("fn main() {{\n    x := {}1{}\n}}\n", "(".repeat(n), ")".repeat(n));
            parse(&src).unwrap_err()
        });
        assert!(err.msg.contains("too deeply nested"), "{}", err.msg);
    }

    #[test]
    fn reasonable_paren_nesting_parses() {
        let n = 64;
        let src = format!("{}1{}", "(".repeat(n), ")".repeat(n));
        assert!(parse_expr(&src).is_ok());
    }

    #[test]
    fn deep_block_nesting_errors_not_crashes() {
        let err = on_main_sized_stack(|| {
            let n = 5_000;
            let mut src = String::from("fn main() {\n");
            for _ in 0..n {
                src.push_str("if true {\n");
            }
            src.push_str("x := 1\n");
            for _ in 0..n {
                src.push_str("}\n");
            }
            src.push_str("}\n");
            parse(&src).unwrap_err()
        });
        assert!(err.msg.contains("too deeply nested"), "{}", err.msg);
    }
}
