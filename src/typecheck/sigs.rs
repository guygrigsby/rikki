//! What the stdlib looks like to the checker: module signatures
//! and the container method table. The runtime twin is builtins.rs
//! and src/stdlib/; golden tests keep the two in agreement.

use super::*;

pub(super) const STD_MODULES: &[&str] = &["math", "error", "file", "ctx", "http"];

pub(super) enum Member {
    Fn(Vec<Type>, Vec<Type>),
    Const(Type),
}

pub(super) fn err_opt() -> Type {
    Type::Opt(Box::new(Type::Error))
}

pub(super) fn std_member(module: &str, name: &str) -> Option<Member> {
    use Type::*;
    let ctx = || Struct("Ctx".into());
    let resp = || Struct("Response".into());
    let m = match (module, name) {
        ("math", "sqrt") | ("math", "cos") | ("math", "sin") | ("math", "tan") => {
            Member::Fn(vec![Float], vec![Float])
        }
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
        ("http", "request") => Member::Fn(
            vec![ctx(), Struct("Request".into())],
            vec![resp(), err_opt()],
        ),
        ("http", "stream") => Member::Fn(
            vec![ctx(), Str, Str, Fn(vec![Str], vec![])],
            vec![resp(), err_opt()],
        ),
        _ => return None,
    };
    Some(m)
}

impl Checker {
    pub(super) fn container_method(
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
            (Str, "find") => {
                self.check_args(&[Str], args, line, col);
                one(Opt(Box::new(Int)))
            }
            (Str, "fields" | "lines" | "chars") => {
                self.check_args(&[], args, line, col);
                one(List(Box::new(Str)))
            }
            (Str, "trim_prefix" | "trim_suffix") => {
                self.check_args(&[Str], args, line, col);
                one(Str)
            }
            (Str, "repeat") => {
                self.check_args(&[Int], args, line, col);
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
                    self.diag(line, col, format!("sum needs []int or []float, got {recv}"));
                }
                one((**elem).clone())
            }
            (List(elem), "sorted") => {
                self.check_args(&[], args, line, col);
                if !matches!(**elem, Int | Float | Str | Unknown) {
                    self.diag(
                        line,
                        col,
                        format!("sorted needs comparable elements, got {recv}"),
                    );
                }
                one(recv.clone())
            }
            (List(elem), "sorted_by") => {
                let f = Fn(vec![(**elem).clone(), (**elem).clone()], vec![Bool]);
                self.args_with_fn(&f, args, line, col);
                one(recv.clone())
            }
            (List(elem), "contains") => {
                self.check_args(&[(**elem).clone()], args, line, col);
                one(Bool)
            }
            (List(elem), "join") => {
                if !matches!(**elem, Str | Unknown) {
                    self.diag(line, col, format!("join needs []str, got {recv}"));
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
}
