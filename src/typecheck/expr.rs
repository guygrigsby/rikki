//! Expression checking: check_expr and its helpers (binary ops,
//! calls, methods, fields, lambdas, format strings).

use super::*;

impl Checker {
    // ---------- expressions ----------

    pub(super) fn expr_one(&mut self, e: &Expr, expected: Option<&Type>) -> Type {
        match self.check_expr(e, expected) {
            ExprTy::One(t) => t,
            ExprTy::Multi(_) => {
                self.diag(e.span, "multiple values in single-value context");
                Type::Unknown
            }
            ExprTy::PyChain => {
                self.diag(e.span, "error result must be handled");
                Type::Py
            }
        }
    }

    /// Like expr_one but lets a py chain through as `py` (for contexts that
    /// absorb its fallibility: conversions, operators, further chain links).
    pub(super) fn expr_pyish(&mut self, e: &Expr, expected: Option<&Type>) -> Type {
        match self.check_expr(e, expected) {
            ExprTy::One(t) => t,
            ExprTy::Multi(_) => {
                self.diag(e.span, "multiple values in single-value context");
                Type::Unknown
            }
            ExprTy::PyChain => Type::Py,
        }
    }

    pub(super) fn check_expr(&mut self, e: &Expr, expected: Option<&Type>) -> ExprTy {
        use ExprKind as K;
        let one = ExprTy::One;
        let span = e.span;
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
                self.diag(span, format!("undefined: {n}"));
                one(Type::Unknown)
            }
            K::List(items) => {
                let expected_elem = match expected {
                    Some(Type::List(t)) => Some((**t).clone()),
                    _ => None,
                };
                if items.is_empty() && expected_elem.is_none() {
                    self.diag(
                        span,
                        "cannot infer element type of []; use it where a list type is expected",
                    );
                }
                let mut elem = expected_elem.unwrap_or(Type::Unknown);
                for it in items {
                    let t = self.expr_one(it, Some(&elem));
                    if elem == Type::Unknown {
                        elem = t;
                    } else if !elem.accepts(&t) {
                        self.diag(it.span, format!("expected {elem}, got {t}"));
                    }
                }
                one(Type::List(Box::new(elem)))
            }
            K::ListLit { elem, items } => {
                let et = self.resolve(elem, span);
                for it in items {
                    let t = self.expr_one(it, Some(&et));
                    if !et.accepts(&t) {
                        self.diag(it.span, format!("expected {et}, got {t}"));
                    }
                }
                one(Type::List(Box::new(et)))
            }
            K::MapLit { key, val, entries } => {
                let kt = self.resolve(key, span);
                let vt = self.resolve(val, span);
                for (k, v) in entries {
                    let got_k = self.expr_one(k, Some(&kt));
                    if !kt.accepts(&got_k) {
                        self.diag(k.span, format!("expected {kt}, got {got_k}"));
                    }
                    let got_v = self.expr_one(v, Some(&vt));
                    if !vt.accepts(&got_v) {
                        self.diag(v.span, format!("expected {vt}, got {got_v}"));
                    }
                }
                one(Type::Map(Box::new(kt), Box::new(vt)))
            }
            K::StructLit { name, fields } => {
                // Ctx is opaque: the checker knows it as a struct, the
                // interpreter does not; only the ctx module makes one
                if name == "Ctx" && matches!(self.imports.get("ctx"), Some(ImportKind::Std(_))) {
                    self.diag(span, "Ctx cannot be constructed; use ctx.background()");
                    return one(Type::Struct(name.clone()));
                }
                let Some(def) = self.structs.get(name).cloned() else {
                    self.diag(span, format!("unknown struct: {name}"));
                    return one(Type::Unknown);
                };
                for (fname, fty) in &def {
                    match fields.iter().find(|(n, _)| n == fname) {
                        Some((_, v)) => {
                            let t = self.expr_one(v, Some(fty));
                            if !fty.accepts(&t) {
                                self.diag(v.span, format!("expected {fty}, got {t}"));
                            }
                        }
                        None => self.diag(span, format!("missing field: {fname}")),
                    }
                }
                for (fname, _) in fields {
                    if !def.iter().any(|(n, _)| n == fname) {
                        self.diag(span, format!("unknown field: {fname}"));
                    }
                }
                one(Type::Struct(name.clone()))
            }
            K::Unary { op, rhs } => {
                let t = self.expr_one(rhs, None);
                match op {
                    UnOp::Not => {
                        if !matches!(t, Type::Bool | Type::Unknown) {
                            self.diag(span, format!("! needs bool, got {t}"));
                        }
                        one(Type::Bool)
                    }
                    UnOp::Neg => {
                        if !matches!(t, Type::Int | Type::Float | Type::Unknown) {
                            self.diag(span, format!("- needs int or float, got {t}"));
                        }
                        one(t)
                    }
                }
            }
            K::Binary { op, lhs, rhs } => {
                let t = self.binary(*op, lhs, rhs, span);
                if t == Type::Py {
                    ExprTy::PyChain
                } else {
                    one(t)
                }
            }
            K::Call {
                callee,
                args,
                kwargs,
            } => {
                let ty = self.call(callee, args, span);
                self.check_kwargs(kwargs, &ty, span);
                ty
            }
            K::Method {
                recv,
                name,
                args,
                kwargs,
            } => {
                let ty = self.method(recv, name, args, span);
                self.check_kwargs(kwargs, &ty, span);
                ty
            }
            K::Field { recv, name } => {
                let t = self.field(recv, name, span);
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
                            self.diag(idx.span, format!("index must be int, got {it}"));
                        }
                        one(*t)
                    }
                    Type::Map(k, v) => {
                        let it = self.expr_one(idx, Some(&k));
                        if !k.accepts(&it) {
                            self.diag(idx.span, format!("expected {k}, got {it}"));
                        }
                        one(Type::Opt(v))
                    }
                    Type::Str => {
                        let it = self.expr_one(idx, Some(&Type::Int));
                        if !matches!(it, Type::Int | Type::Unknown) {
                            self.diag(idx.span, format!("index must be int, got {it}"));
                        }
                        one(Type::Str)
                    }
                    Type::Unknown => one(Type::Unknown),
                    t => {
                        self.diag(span, format!("cannot index {t}"));
                        one(Type::Unknown)
                    }
                }
            }
            K::Slice { recv, lo, hi } => {
                let rt = self.expr_one(recv, None);
                for b in [lo, hi] {
                    let t = self.expr_one(b, Some(&Type::Int));
                    if !matches!(t, Type::Int | Type::Unknown) {
                        self.diag(b.span, format!("slice bound must be int, got {t}"));
                    }
                }
                match rt {
                    Type::List(_) | Type::Str | Type::Unknown => one(rt),
                    t => {
                        self.diag(span, format!("cannot slice {t}"));
                        one(Type::Unknown)
                    }
                }
            }
            K::Lambda { params, ret, body } => one(self.lambda(params, ret, body, expected, span)),
            K::Check(inner) => {
                if self.current_ret.last() != Some(&err_opt()) {
                    self.diag(span, "check requires enclosing function to return error?");
                }
                let ty = self.check_expr(inner, None);
                let parts = match ty {
                    ExprTy::One(t) => vec![t],
                    ExprTy::Multi(ts) => ts,
                    ExprTy::PyChain => vec![Type::Py, err_opt()],
                };
                if parts.last() != Some(&err_opt()) && parts.last() != Some(&Type::Unknown) {
                    self.diag(span, "check needs a fallible expression");
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
                let t = self.resolve(target, span);
                let at = self.expr_pyish(arg, None);
                let ok = matches!(
                    (&t, &at),
                    (_, Type::Py)
                        | (_, Type::Unknown)
                        | (Type::Int | Type::Float, Type::Int | Type::Float | Type::Str)
                        | (Type::Bool, Type::Str | Type::Bool)
                        | (Type::Str, _)
                        | (Type::List(_), Type::List(_))
                );
                if !ok {
                    self.diag(span, format!("cannot convert {at} to {t}"));
                }
                // fallible only from py (bridge) or str (parse); numeric,
                // identity, str-render, and list pass-through are single-valued
                let fallible = matches!(
                    (&t, &at),
                    (_, Type::Py)
                        | (_, Type::Unknown)
                        | (Type::Int | Type::Float | Type::Bool, Type::Str)
                );
                if fallible {
                    ExprTy::Multi(vec![t, err_opt()])
                } else {
                    ExprTy::One(t)
                }
            }
        }
    }

    fn binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span) -> Type {
        let lt = self.expr_pyish(lhs, None);
        let rt = self.expr_pyish(rhs, Some(&lt));
        if lt == Type::Py || rt == Type::Py {
            if matches!(op, BinOp::And | BinOp::Or) {
                self.diag(span, "&& and || need bool, got py");
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
                    self.diag(span, "int and float do not mix");
                    return Type::Unknown;
                }
                match (&lt, &rt, op) {
                    (Type::Int, Type::Int, _) => Type::Int,
                    (Type::Float, Type::Float, BinOp::Rem) => {
                        self.diag(span, "% needs int operands");
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
                            self.diag(span, format!("cannot concat list[{a}] and list[{b}]"));
                            lt.clone()
                        }
                    }
                    _ => {
                        self.diag(span, format!("cannot apply operator to {lt} and {rt}"));
                        Type::Unknown
                    }
                }
            }
            BinOp::Lt | BinOp::LtEq | BinOp::Gt | BinOp::GtEq => {
                if !unknown {
                    let ok = matches!(
                        (&lt, &rt),
                        (Type::Int, Type::Int)
                            | (Type::Float, Type::Float)
                            | (Type::Str, Type::Str)
                    );
                    if !ok {
                        self.diag(span, format!("cannot compare {lt} and {rt}"));
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
                        self.diag(span, "none only compares to option types");
                    }
                } else if !unknown {
                    let comparable =
                        matches!(&lt, Type::Int | Type::Float | Type::Str | Type::Bool) && lt == rt;
                    if !comparable {
                        self.diag(span, format!("cannot compare {lt} and {rt}"));
                    }
                }
                Type::Bool
            }
            BinOp::And | BinOp::Or => {
                for (t, e) in [(&lt, lhs), (&rt, rhs)] {
                    if !matches!(t, Type::Bool | Type::Unknown) {
                        self.diag(e.span, format!("&& and || need bool, got {t}"));
                    }
                }
                Type::Bool
            }
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> ExprTy {
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
                            self.diag(span, format!("{name} needs a format string"));
                        } else {
                            let t = self.expr_one(&args[0], Some(&Type::Str));
                            if !matches!(t, Type::Str | Type::Unknown) {
                                self.diag(span, format!("{name} format must be str, got {t}"));
                            }
                            let arg_tys: Vec<Type> =
                                args[1..].iter().map(|a| self.expr_one(a, None)).collect();
                            // a literal format is verified here; anything
                            // else stays a runtime check
                            if let ExprKind::Str(fmt) = &args[0].kind {
                                self.check_format(name, fmt, &arg_tys, span);
                            }
                        }
                        return ExprTy::One(if name == "sprintf" {
                            Type::Str
                        } else {
                            Type::Unit
                        });
                    }
                    "append" => {
                        if args.is_empty() {
                            self.diag(span, "append takes a list and values");
                            return ExprTy::One(Type::Unknown);
                        }
                        let t0 = self.expr_one(&args[0], None);
                        return match t0 {
                            Type::List(elem) => {
                                for a in &args[1..] {
                                    let t = self.expr_one(a, Some(&elem));
                                    if !elem.accepts(&t) {
                                        self.diag(a.span, format!("expected {elem}, got {t}"));
                                    }
                                }
                                ExprTy::One(Type::List(elem))
                            }
                            Type::Unknown => {
                                for a in &args[1..] {
                                    self.expr_one(a, None);
                                }
                                ExprTy::One(Type::Unknown)
                            }
                            _ => {
                                self.diag(span, format!("append needs a list, got {t0}"));
                                for a in &args[1..] {
                                    self.expr_one(a, None);
                                }
                                ExprTy::One(Type::Unknown)
                            }
                        };
                    }
                    "ord" => {
                        if args.len() != 1 {
                            self.diag(span, "ord takes one argument");
                        } else {
                            let t = self.expr_one(&args[0], Some(&Type::Str));
                            if !matches!(t, Type::Str | Type::Unknown) {
                                self.diag(span, format!("ord needs str, got {t}"));
                            }
                        }
                        return ExprTy::One(Type::Int);
                    }
                    "chr" => {
                        if args.len() != 1 {
                            self.diag(span, "chr takes one argument");
                        } else {
                            let t = self.expr_one(&args[0], Some(&Type::Int));
                            if !matches!(t, Type::Int | Type::Unknown) {
                                self.diag(span, format!("chr needs int, got {t}"));
                            }
                        }
                        return ExprTy::One(Type::Str);
                    }
                    "len" => {
                        if args.len() != 1 {
                            self.diag(span, "len takes one argument");
                        } else {
                            let t = self.expr_one(&args[0], None);
                            if !matches!(
                                t,
                                Type::Str | Type::List(_) | Type::Map(..) | Type::Unknown
                            ) {
                                self.diag(span, format!("len needs str, list, or map, got {t}"));
                            }
                        }
                        return ExprTy::One(Type::Int);
                    }
                    "args" => {
                        if !args.is_empty() {
                            self.diag(span, "args takes no arguments");
                        }
                        return ExprTy::One(Type::List(Box::new(Type::Str)));
                    }
                    "input" => {
                        if args.len() != 1 {
                            self.diag(span, "input takes one str prompt");
                        } else {
                            let t = self.expr_one(&args[0], Some(&Type::Str));
                            if !matches!(t, Type::Str | Type::Unknown) {
                                self.diag(span, format!("input prompt must be str, got {t}"));
                            }
                        }
                        return ExprTy::Multi(vec![Type::Str, err_opt()]);
                    }
                    _ => {}
                }
            }
        }
        let ct = self.expr_pyish(callee, None);
        if ct == Type::Py {
            // a py chain argument is absorbed into this chain
            for a in args {
                self.expr_pyish(a, None);
            }
            return ExprTy::PyChain;
        }
        match ct {
            Type::Fn(params, rets) => {
                self.check_args(&params, args, span);
                rets_ty(rets)
            }
            Type::Unknown => ExprTy::One(Type::Unknown),
            t => {
                self.diag(span, format!("not callable: {t}"));
                ExprTy::One(Type::Unknown)
            }
        }
    }

    /// Statically checks a literal printf/sprintf format against the
    /// argument types. Mirrors the runtime verb table.
    fn check_format(&mut self, name: &str, fmt: &str, arg_tys: &[Type], span: Span) {
        let pieces = match crate::fmt::parse(fmt) {
            Ok(p) => p,
            Err(e) => {
                self.diag(span, e.msg(name));
                return;
            }
        };
        let verbs: Vec<char> = pieces
            .iter()
            .filter_map(|p| match p {
                crate::fmt::Piece::Verb { verb, .. } => Some(*verb),
                crate::fmt::Piece::Lit(_) => None,
            })
            .collect();
        if verbs.len() != arg_tys.len() {
            self.diag(
                span,
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
                    self.diag(span, format!("{name}: unknown verb %{v}"));
                    continue;
                }
            };
            if *t != want {
                self.diag(span, format!("{name}: %{v} needs {want}, got {t}"));
            }
        }
    }

    /// Named arguments exist only for python calls: the call's result is a
    /// py chain exactly when the callee was py, so gate on that.
    fn check_kwargs(&mut self, kwargs: &[(String, Expr)], result: &ExprTy, span: Span) {
        if kwargs.is_empty() {
            return;
        }
        if !matches!(result, ExprTy::PyChain) {
            self.diag(span, "named arguments are only for python calls");
            for (_, v) in kwargs {
                self.expr_one(v, None);
            }
            return;
        }
        for (_, v) in kwargs {
            self.expr_pyish(v, None);
        }
    }

    pub(super) fn check_args(&mut self, params: &[Type], args: &[Expr], span: Span) {
        if params.len() != args.len() {
            self.diag(
                span,
                format!("expected {} arguments, got {}", params.len(), args.len()),
            );
        }
        for (p, a) in params.iter().zip(args) {
            let t = self.expr_one(a, Some(p));
            if !p.accepts(&t) {
                self.diag(a.span, format!("expected {p}, got {t}"));
            }
        }
    }

    fn method(&mut self, recv: &Expr, name: &str, args: &[Expr], span: Span) -> ExprTy {
        // error.new / error.wrap: `error` is a type name acting as a module
        if let ExprKind::Ident(id) = &recv.kind {
            if id == "error" && self.lookup(id).is_none() {
                return match name {
                    "new" => {
                        self.check_args(&[Type::Str], args, span);
                        ExprTy::One(Type::Error)
                    }
                    "wrap" => {
                        self.check_args(&[Type::Error, Type::Str], args, span);
                        ExprTy::One(Type::Error)
                    }
                    _ => {
                        self.diag(span, format!("error has no member {name}"));
                        ExprTy::One(Type::Unknown)
                    }
                };
            }
        }
        let rt = self.expr_pyish(recv, None);
        if rt == Type::Py {
            // a py chain argument is absorbed into this chain
            for a in args {
                self.expr_pyish(a, None);
            }
            return ExprTy::PyChain;
        }
        match &rt {
            Type::Module(m) if matches!(self.imports.get(m), Some(ImportKind::File(_))) => {
                let mangled = crate::loader::qualified(m, name);
                match self.fns.get(&mangled).cloned() {
                    Some((params, rets)) => {
                        self.check_args(&params, args, span);
                        rets_ty(rets)
                    }
                    None => {
                        self.diag(span, format!("{m} has no member {name}"));
                        ExprTy::One(Type::Unknown)
                    }
                }
            }
            Type::Module(m) => {
                // polymorphic math members
                if m == "math" && matches!(name, "abs" | "min" | "max") {
                    let want = if name == "abs" { 1 } else { 2 };
                    if args.len() != want {
                        self.diag(span, format!("math.{name} takes {want} arguments"));
                        return ExprTy::One(Type::Unknown);
                    }
                    let t0 = self.expr_one(&args[0], None);
                    if !matches!(t0, Type::Int | Type::Float | Type::Unknown) {
                        self.diag(span, format!("math.{name} needs int or float, got {t0}"));
                    }
                    for a in &args[1..] {
                        let t = self.expr_one(a, Some(&t0));
                        if !t0.accepts(&t) {
                            self.diag(a.span, format!("expected {t0}, got {t}"));
                        }
                    }
                    return ExprTy::One(t0);
                }
                match std_member(m, name) {
                    Some(Member::Fn(params, rets)) => {
                        self.check_args(&params, args, span);
                        rets_ty(rets)
                    }
                    Some(Member::Const(_)) => {
                        self.diag(span, format!("{m}.{name} is not callable"));
                        ExprTy::One(Type::Unknown)
                    }
                    None => {
                        self.diag(span, format!("{m} has no member {name}"));
                        ExprTy::One(Type::Unknown)
                    }
                }
            }
            Type::Struct(s) if s == "Ctx" => match name {
                "done" => {
                    self.check_args(&[], args, span);
                    ExprTy::One(Type::Bool)
                }
                "err" => {
                    self.check_args(&[], args, span);
                    ExprTy::One(err_opt())
                }
                _ => {
                    self.diag(span, format!("Ctx has no method {name}"));
                    ExprTy::One(Type::Unknown)
                }
            },
            Type::Str | Type::List(_) | Type::Map(..) => {
                self.container_method(&rt, name, args, span)
            }
            Type::Opt(_) => {
                self.diag(
                    span,
                    format!("value might be none; check it before calling {name}"),
                );
                ExprTy::One(Type::Unknown)
            }
            Type::Unknown => ExprTy::One(Type::Unknown),
            t => {
                self.diag(span, format!("{t} has no method {name}"));
                ExprTy::One(Type::Unknown)
            }
        }
    }
    /// Check args against a single expected fn param; returns the arg's
    /// (possibly inferred) type so callers can read the lambda's return.
    pub(super) fn args_with_fn(&mut self, f: &Type, args: &[Expr], span: Span) -> Option<Type> {
        if args.len() != 1 {
            self.diag(span, "expected one function argument");
            return None;
        }
        let t = self.expr_one(&args[0], Some(f));
        if let Type::Fn(want, _) = f {
            if let Type::Fn(got, _) = &t {
                if want.len() != got.len() {
                    self.diag(span, format!("expected {f}, got {t}"));
                }
            } else if t != Type::Unknown {
                self.diag(span, format!("expected {f}, got {t}"));
            }
        }
        Some(t)
    }

    fn field(&mut self, recv: &Expr, name: &str, span: Span) -> Type {
        let rt = self.expr_pyish(recv, None);
        if rt == Type::Py {
            return Type::Py;
        }
        match &rt {
            Type::Struct(s) => match self
                .structs
                .get(s)
                .and_then(|fs| fs.iter().find(|(f, _)| f == name).map(|(_, t)| t.clone()))
            {
                Some(t) => t,
                None => {
                    self.diag(span, format!("{s} has no field {name}"));
                    Type::Unknown
                }
            },
            Type::Error => match name {
                "msg" | "pytype" | "traceback" => Type::Str,
                "cause" => err_opt(),
                _ => {
                    self.diag(span, format!("error has no field {name}"));
                    Type::Unknown
                }
            },
            Type::Module(m) if matches!(self.imports.get(m), Some(ImportKind::File(_))) => {
                match self.fns.get(&crate::loader::qualified(m, name)) {
                    Some(_) => {
                        self.diag(
                            span,
                            format!("module functions are not first class in v1; call {m}.{name}(...) directly"),
                        );
                        Type::Unknown
                    }
                    None => {
                        self.diag(span, format!("{m} has no member {name}"));
                        Type::Unknown
                    }
                }
            }
            Type::Module(m) => match std_member(m, name) {
                Some(Member::Const(t)) => t,
                Some(Member::Fn(..)) => {
                    self.diag(
                        span,
                        format!("module functions are not first class in v1; call {m}.{name}(...) directly"),
                    );
                    Type::Unknown
                }
                None => {
                    self.diag(span, format!("{m} has no member {name}"));
                    Type::Unknown
                }
            },
            Type::Opt(_) => {
                self.diag(
                    span,
                    format!("value might be none; check it before using .{name}"),
                );
                Type::Unknown
            }
            Type::Unknown => Type::Unknown,
            t => {
                self.diag(span, format!("{t} has no field {name}"));
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
        span: Span,
    ) -> Type {
        let expected_fn = match expected {
            Some(Type::Fn(a, r)) => Some((a.clone(), r.clone())),
            _ => None,
        };
        let mut param_tys = vec![];
        for (i, p) in params.iter().enumerate() {
            let t = match &p.ty {
                Some(t) => self.resolve(t, span),
                None => match expected_fn.as_ref().and_then(|(a, _)| a.get(i)) {
                    Some(t) => t.clone(),
                    None => {
                        self.diag(
                            span,
                            format!("lambda parameter {} needs a type here", p.name),
                        );
                        Type::Unknown
                    }
                },
            };
            param_tys.push(t);
        }
        let declared_ret: Option<Vec<Type>> = ret
            .as_ref()
            .map(|rs| rs.iter().map(|t| self.resolve(t, span)).collect());

        let saved_ret = std::mem::take(&mut self.current_ret);
        let saved_loop = std::mem::replace(&mut self.loop_depth, 0);
        self.push_scope();
        for (p, t) in params.iter().zip(&param_tys) {
            self.declare(&p.name, t.clone(), span);
        }

        let ret_tys: Vec<Type> = if let Some(rs) = declared_ret {
            self.current_ret = rs.clone();
            let diverges = self.check_block_inline(body);
            if !rs.is_empty() && !diverges {
                self.diag(span, "missing return in lambda");
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
