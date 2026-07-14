#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    /// 0..=255 scalar; compare-only in v1 (design 2026-07-13, ADR 0021).
    Byte,
    Float,
    Bool,
    Str,
    List(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Opt(Box<Type>),
    Fn(Vec<Type>, Vec<Type>),
    Struct(String),
    Error,
    Py,
    Module(String),
    Unit,
    /// Element type of an empty list literal before context fixes it.
    Unknown,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Byte => write!(f, "byte"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::List(t) => write!(f, "[]{t}"),
            Type::Map(k, v) => write!(f, "map[{k}]{v}"),
            Type::Opt(t) => write!(f, "{t}?"),
            Type::Fn(args, rets) => {
                write!(f, "fn(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")?;
                match rets.len() {
                    0 => Ok(()),
                    1 => write!(f, " {}", rets[0]),
                    _ => {
                        write!(f, " (")?;
                        for (i, r) in rets.iter().enumerate() {
                            if i > 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{r}")?;
                        }
                        write!(f, ")")
                    }
                }
            }
            Type::Struct(n) => write!(f, "{n}"),
            Type::Error => write!(f, "error"),
            Type::Py => write!(f, "py"),
            Type::Module(n) => write!(f, "module {n}"),
            Type::Unit => write!(f, "()"),
            Type::Unknown => write!(f, "?"),
        }
    }
}

impl Type {
    /// `value` is usable where `self` is expected. Widens T into T? and
    /// unifies Unknown (empty list literal) with anything.
    pub fn accepts(&self, value: &Type) -> bool {
        if self == value {
            return true;
        }
        match (self, value) {
            (_, Type::Unknown) | (Type::Unknown, _) => true,
            (Type::Opt(inner), v) => {
                inner.accepts(v) || matches!(v, Type::Opt(x) if inner.accepts(x))
            }
            (Type::List(a), Type::List(b)) => a.accepts(b),
            (Type::Map(ak, av), Type::Map(bk, bv)) => ak.accepts(bk) && av.accepts(bv),
            _ => false,
        }
    }
}
