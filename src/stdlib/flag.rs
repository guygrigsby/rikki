//! CLI flags (spec 15.11). Data-shaped, no registry: constructors make
//! Flag values, parse is a pure function over argv, and help is an
//! error value carrying the usage text. The module never writes output
//! and never exits; main decides (ADR 0012 applied to CLI UX).

use indexmap::IndexMap;

use crate::interp::{Fault, Interp};
use crate::value::{ErrVal, MapKey, Value};

pub(crate) fn struct_types() -> Vec<(String, Vec<(String, crate::types::Type)>)> {
    use crate::types::Type;
    vec![
        (
            "Flag".into(),
            vec![
                ("name".into(), Type::Str),
                ("short".into(), Type::Str),
                ("fallback".into(), Type::Str),
                ("usage".into(), Type::Str),
                ("toggle".into(), Type::Bool),
            ],
        ),
        (
            "Parsed".into(),
            vec![
                (
                    "values".into(),
                    Type::Map(Box::new(Type::Str), Box::new(Type::Str)),
                ),
                ("rest".into(), Type::List(Box::new(Type::Str))),
            ],
        ),
    ]
}

pub(crate) fn struct_exprs() -> Vec<(String, Vec<(String, crate::ast::TypeExpr)>)> {
    use crate::ast::TypeExpr;
    let named = |n: &str| TypeExpr::Named(n.into());
    vec![
        (
            "Flag".into(),
            vec![
                ("name".into(), named("str")),
                ("short".into(), named("str")),
                ("fallback".into(), named("str")),
                ("usage".into(), named("str")),
                ("toggle".into(), named("bool")),
            ],
        ),
        (
            "Parsed".into(),
            vec![
                (
                    "values".into(),
                    TypeExpr::Map(Box::new(named("str")), Box::new(named("str"))),
                ),
                ("rest".into(), TypeExpr::List(Box::new(named("str")))),
            ],
        ),
    ]
}

struct Def {
    name: String,
    short: String,
    fallback: String,
    usage: String,
    toggle: bool,
}

fn flag_value(name: &str, short: &str, fallback: &str, usage: &str, toggle: bool) -> Value {
    let mut fields = IndexMap::new();
    fields.insert("name".to_string(), Value::Str(name.into()));
    fields.insert("short".to_string(), Value::Str(short.into()));
    fields.insert("fallback".to_string(), Value::Str(fallback.into()));
    fields.insert("usage".to_string(), Value::Str(usage.into()));
    fields.insert("toggle".to_string(), Value::Bool(toggle));
    Value::Struct {
        name: "Flag".into(),
        fields,
    }
}

fn parsed_value(values: IndexMap<MapKey, Value>, rest: Vec<Value>) -> Value {
    let mut fields = IndexMap::new();
    fields.insert("values".to_string(), Value::map(values));
    fields.insert("rest".to_string(), Value::list(rest));
    Value::Struct {
        name: "Parsed".into(),
        fields,
    }
}

fn fail(msg: String) -> Value {
    Value::Tuple(vec![
        parsed_value(IndexMap::new(), vec![]),
        Value::Err(ErrVal {
            msg,
            ..Default::default()
        }),
    ])
}

fn usage_text(defs: &[Def]) -> String {
    let head = |d: &Def| {
        if d.short.is_empty() {
            format!("--{}", d.name)
        } else {
            format!("-{}, --{}", d.short, d.name)
        }
    };
    let width = defs.iter().map(|d| head(d).chars().count()).max().unwrap_or(0);
    let mut out = String::from("flags:");
    for d in defs {
        let h = head(d);
        let pad = " ".repeat(width - h.chars().count() + 2);
        out.push_str(&format!("\n  {h}{pad}{}", d.usage));
        if !d.toggle {
            out.push_str(&format!(" (default {})", d.fallback));
        }
    }
    out
}

fn str_field(v: &Value, f: &str) -> Option<String> {
    let Value::Struct { fields, .. } = v else {
        return None;
    };
    match fields.get(f) {
        Some(Value::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn defs_from(flags: &[Value]) -> Result<Vec<Def>, String> {
    let mut defs = vec![];
    for v in flags {
        let toggle = matches!(
            v,
            Value::Struct { fields, .. } if matches!(fields.get("toggle"), Some(Value::Bool(true)))
        );
        let d = Def {
            name: str_field(v, "name").ok_or("flag.parse: bad Flag value")?,
            short: str_field(v, "short").unwrap_or_default(),
            fallback: str_field(v, "fallback").unwrap_or_default(),
            usage: str_field(v, "usage").unwrap_or_default(),
            toggle,
        };
        if d.name.is_empty() {
            return Err("flag.parse: a flag needs a name".into());
        }
        if d.name == "help" || d.short == "h" {
            return Err(format!("flag.parse: --{} collides with the built-in help", d.name));
        }
        if defs
            .iter()
            .any(|e: &Def| e.name == d.name || (!d.short.is_empty() && e.short == d.short))
        {
            return Err(format!("flag.parse: duplicate flag --{}", d.name));
        }
        defs.push(d);
    }
    Ok(defs)
}

fn parse(argv: &[Value], flags: &[Value]) -> Value {
    let argv: Vec<String> = argv
        .iter()
        .filter_map(|v| match v {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    let defs = match defs_from(flags) {
        Ok(d) => d,
        Err(m) => return fail(m),
    };
    let usage = usage_text(&defs);

    let mut values: IndexMap<MapKey, Value> = defs
        .iter()
        .map(|d| (MapKey::Str(d.name.clone()), Value::Str(d.fallback.clone())))
        .collect();
    let mut rest: Vec<Value> = vec![];

    let mut i = 0;
    while i < argv.len() {
        let t = &argv[i];
        if t == "--" {
            rest.extend(argv[i + 1..].iter().cloned().map(Value::Str));
            break;
        }
        let (body, long) = if let Some(b) = t.strip_prefix("--") {
            (b, true)
        } else if t.len() > 1 && t.starts_with('-') {
            (&t[1..], false)
        } else {
            rest.extend(argv[i..].iter().cloned().map(Value::Str));
            break;
        };
        let (key, inline) = match body.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (body, None),
        };
        if (long && key == "help") || (!long && key == "h") {
            return fail(usage);
        }
        let dash = if long { "--" } else { "-" };
        let Some(d) = defs
            .iter()
            .find(|d| if long { d.name == key } else { d.short == key })
        else {
            return fail(format!("unknown flag {dash}{key}\n{usage}"));
        };
        let val = if d.toggle {
            if inline.is_some() {
                return fail(format!(
                    "flag --{} is a toggle and takes no value\n{usage}",
                    d.name
                ));
            }
            "true".to_string()
        } else if let Some(v) = inline {
            v
        } else {
            i += 1;
            match argv.get(i) {
                Some(v) => v.clone(),
                None => return fail(format!("flag --{} needs a value\n{usage}", d.name)),
            }
        };
        values.insert(MapKey::Str(d.name.clone()), Value::Str(val));
        i += 1;
    }
    Value::Tuple(vec![parsed_value(values, rest), Value::NoneV])
}

/// The never-miss read: parse filled every declared flag, so a lookup
/// by declared name is total; an undeclared name reads "".
fn get(p: &Value, name: &str) -> Value {
    let Value::Struct { fields, .. } = p else {
        return Value::Str(String::new());
    };
    if let Some(Value::Map(m)) = fields.get("values") {
        if let Some(Value::Str(s)) = m.borrow().get(&MapKey::Str(name.to_string())) {
            return Value::Str(s.clone());
        }
    }
    Value::Str(String::new())
}

pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    let v = match (name, args.as_slice()) {
        ("value", [Value::Str(n), Value::Str(s), Value::Str(f), Value::Str(u)]) => {
            flag_value(n, s, f, u, false)
        }
        ("toggle", [Value::Str(n), Value::Str(s), Value::Str(u)]) => {
            flag_value(n, s, "false", u, true)
        }
        ("parse", [Value::List(argv), Value::List(flags)]) => {
            parse(&argv.borrow(), &flags.borrow())
        }
        ("get", [p @ Value::Struct { .. }, Value::Str(n)]) => get(p, n),
        _ => return Err(interp.fault(format!("flag.{name}: bad arguments"))),
    };
    Ok(v)
}
