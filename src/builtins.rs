use crate::ast::TypeExpr;
use crate::interp::{Fault, Interp};
use crate::value::{render, ErrVal, Value};

impl Interp<'_> {
    pub(crate) fn builtin_call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
        match name {
            "print" => {
                let line = args.iter().map(render).collect::<Vec<_>>().join(" ");
                self.out.push_str(&line);
                self.out.push_str("\n");
                Ok(Value::Unit)
            }
            "printf" | "sprintf" => {
                let Some(Value::Str(fmt)) = args.first() else {
                    return Err(self.fault(format!("{name} needs a format string")));
                };
                let s = self.format(fmt, &args[1..])?;
                if name == "printf" {
                    self.out.push_str(&s);
                    Ok(Value::Unit)
                } else {
                    Ok(Value::Str(s))
                }
            }
            "len" => match args.first() {
                Some(Value::Str(s)) => Ok(Value::Int(s.chars().count() as i64)),
                Some(Value::List(v)) => Ok(Value::Int(v.len() as i64)),
                Some(Value::Map(m)) => Ok(Value::Int(m.len() as i64)),
                _ => Err(self.fault("len needs str, list, or map")),
            },
            "append" => {
                let mut it = args.into_iter();
                match it.next() {
                    Some(Value::List(mut items)) => {
                        items.extend(it);
                        Ok(Value::List(items))
                    }
                    _ => Err(self.fault("append needs a list first argument")),
                }
            }
            "ord" => match args.first() {
                Some(Value::Str(s)) => {
                    let mut it = s.chars();
                    match (it.next(), it.next()) {
                        (Some(c), None) => Ok(Value::Int(c as i64)),
                        _ => Err(self.fault("ord needs a single character")),
                    }
                }
                _ => Err(self.fault("ord needs a str")),
            },
            "chr" => match args.first() {
                Some(Value::Int(n)) => u32::try_from(*n)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| Value::Str(c.to_string()))
                    .ok_or_else(|| self.fault(format!("chr: invalid code point {n}"))),
                _ => Err(self.fault("chr needs an int")),
            },
            "args" => Ok(Value::List(
                self.prog_args.iter().cloned().map(Value::Str).collect(),
            )),
            "input" => {
                let Some(Value::Str(prompt)) = args.first() else {
                    return Err(self.fault("input needs a str prompt"));
                };
                self.out.push_str(prompt);
                let mut line = String::new();
                use std::io::BufRead;
                match std::io::stdin().lock().read_line(&mut line) {
                    Ok(0) => Ok(Value::Tuple(vec![
                        Value::Str(String::new()),
                        Value::Err(ErrVal {
                            msg: "eof".into(),
                            ..Default::default()
                        }),
                    ])),
                    Ok(_) => {
                        let line = line.trim_end_matches(['\n', '\r']).to_string();
                        Ok(Value::Tuple(vec![Value::Str(line), Value::NoneV]))
                    }
                    Err(e) => Ok(Value::Tuple(vec![
                        Value::Str(String::new()),
                        Value::Err(ErrVal {
                            msg: format!("stdin: {e}"),
                            ..Default::default()
                        }),
                    ])),
                }
            }
            _ => Err(self.fault(format!("unknown function: {name}"))),
        }
    }

    /// Conversions. py and str sources yield (T, error?); other pairs are
    /// infallible and yield the bare value, matching the checker.
    pub(crate) fn convert(&mut self, target: &TypeExpr, v: Value) -> Result<Value, Fault> {
        let ok = |v| Ok(Value::Tuple(vec![v, Value::NoneV]));
        let fail = |this: &Self, t: &TypeExpr, msg: String| {
            Ok(Value::Tuple(vec![
                this.zero(t)?,
                Value::Err(ErrVal {
                    msg,
                    ..Default::default()
                }),
            ]))
        };
        let name = match target {
            TypeExpr::Named(n) => n.as_str(),
            TypeExpr::List(_) => "list",
            _ => return Err(self.fault("bad conversion target")),
        };
        // py extraction goes through the bridge
        if let Value::Py(h) = &v {
            let spec = match target {
                TypeExpr::Named(n) => n.clone(),
                TypeExpr::List(inner) => match &**inner {
                    TypeExpr::Named(n) => format!("list:{n}"),
                    _ => return Err(self.fault("unsupported list conversion from py")),
                },
                _ => return Err(self.fault("bad conversion target")),
            };
            return Ok(match crate::bridge::extract(&spec, h) {
                Ok(val) => Value::Tuple(vec![val, Value::NoneV]),
                Err(e) => Value::Tuple(vec![self.zero(target)?, Value::Err(e)]),
            });
        }
        // infallible pairs return the value bare; str sources keep the
        // (T, error?) tuple the checker promises for them
        match (name, v) {
            ("int", Value::Int(i)) => Ok(Value::Int(i)),
            ("int", Value::Float(f)) => Ok(Value::Int(f as i64)),
            ("int", Value::Str(s)) => match s.trim().parse::<i64>() {
                Ok(i) => ok(Value::Int(i)),
                Err(_) => fail(self, target, format!("cannot parse {s:?} as int")),
            },
            ("float", Value::Float(f)) => Ok(Value::Float(f)),
            ("float", Value::Int(i)) => Ok(Value::Float(i as f64)),
            ("float", Value::Str(s)) => match s.trim().parse::<f64>() {
                Ok(f) => ok(Value::Float(f)),
                Err(_) => fail(self, target, format!("cannot parse {s:?} as float")),
            },
            ("bool", Value::Bool(b)) => Ok(Value::Bool(b)),
            ("bool", Value::Str(s)) => match s.trim() {
                "true" => ok(Value::Bool(true)),
                "false" => ok(Value::Bool(false)),
                _ => fail(self, target, format!("cannot parse {s:?} as bool")),
            },
            ("str", v) => Ok(Value::Str(render(&v))),
            ("list", Value::List(items)) => Ok(Value::List(items)),
            // py conversions land with the bridge
            (t, v) => fail(
                self,
                target,
                format!("cannot convert {} to {t}", render(&v)),
            ),
        }
    }

    pub(crate) fn method_call(
        &mut self,
        recv: Value,
        name: &str,
        mut args: Vec<Value>,
    ) -> Result<Value, Fault> {
        match recv {
            Value::Module(m) => self.module_call(&m, name, args),
            Value::Ctx(c) => crate::stdlib::ctx::method(self, &c, name),
            Value::List(items) => self.list_method(items, name, args),
            Value::Str(s) => self.str_method(&s, name, args),
            Value::Map(m) => {
                let arg = args.pop();
                match (name, arg) {
                    ("keys", None) => Ok(Value::List(m.keys().map(|k| k.to_value()).collect())),
                    ("values", None) => Ok(Value::List(m.values().cloned().collect())),
                    ("has", Some(k)) => match crate::value::MapKey::from_value(&k) {
                        Some(key) => Ok(Value::Bool(m.contains_key(&key))),
                        None => Err(self.fault("bad map key")),
                    },
                    ("delete", Some(k)) => match crate::value::MapKey::from_value(&k) {
                        Some(key) => {
                            let mut out = m.clone();
                            out.shift_remove(&key);
                            Ok(Value::Map(out))
                        }
                        None => Err(self.fault("bad map key")),
                    },
                    _ => Err(self.fault(format!("map has no method {name}"))),
                }
            }
            _ => Err(self.fault(format!("no method {name} on this value"))),
        }
    }

    fn list_method(
        &mut self,
        items: Vec<Value>,
        name: &str,
        mut args: Vec<Value>,
    ) -> Result<Value, Fault> {
        match name {
            "map" => {
                let f = args
                    .pop()
                    .ok_or_else(|| self.fault("map needs a function"))?;
                let mut out = vec![];
                for it in items {
                    out.push(self.call_value(&f, vec![it])?);
                }
                Ok(Value::List(out))
            }
            "filter" => {
                let f = args
                    .pop()
                    .ok_or_else(|| self.fault("filter needs a function"))?;
                let mut out = vec![];
                for it in items {
                    if matches!(self.call_value(&f, vec![it.clone()])?, Value::Bool(true)) {
                        out.push(it);
                    }
                }
                Ok(Value::List(out))
            }
            "each" => {
                let f = args
                    .pop()
                    .ok_or_else(|| self.fault("each needs a function"))?;
                for it in items {
                    self.call_value(&f, vec![it])?;
                }
                Ok(Value::Unit)
            }
            "sum" => {
                let mut ints = 0i64;
                let mut floats = 0f64;
                let mut is_float = false;
                for it in &items {
                    match it {
                        Value::Int(i) => {
                            ints = ints
                                .checked_add(*i)
                                .ok_or_else(|| self.fault("integer overflow"))?;
                        }
                        Value::Float(f) => {
                            is_float = true;
                            floats += f;
                        }
                        _ => return Err(self.fault("sum needs numbers")),
                    }
                }
                Ok(if is_float {
                    Value::Float(floats)
                } else {
                    Value::Int(ints)
                })
            }
            "sorted" => {
                let mut out = items;
                let mut err = None;
                out.sort_by(|a, b| {
                    use std::cmp::Ordering;
                    match (a, b) {
                        (Value::Int(x), Value::Int(y)) => x.cmp(y),
                        (Value::Float(x), Value::Float(y)) => {
                            x.partial_cmp(y).unwrap_or(Ordering::Equal)
                        }
                        (Value::Str(x), Value::Str(y)) => x.cmp(y),
                        _ => {
                            err = Some("sorted needs comparable elements");
                            Ordering::Equal
                        }
                    }
                });
                match err {
                    Some(m) => Err(self.fault(m)),
                    None => Ok(Value::List(out)),
                }
            }
            "sorted_by" => {
                let f = args
                    .pop()
                    .ok_or_else(|| self.fault("sorted_by needs a function"))?;
                // ponytail: insertion sort, O(n^2); a comparator-driven merge
                // sort if big lists ever matter
                let mut out: Vec<Value> = vec![];
                for it in items {
                    let mut pos = out.len();
                    for (i, existing) in out.iter().enumerate() {
                        let before = self.call_value(&f, vec![it.clone(), existing.clone()])?;
                        if matches!(before, Value::Bool(true)) {
                            pos = i;
                            break;
                        }
                    }
                    out.insert(pos, it);
                }
                Ok(Value::List(out))
            }
            "contains" => {
                let v = args
                    .pop()
                    .ok_or_else(|| self.fault("contains needs a value"))?;
                Ok(Value::Bool(items.iter().any(|it| it.eq_value(&v))))
            }
            "join" => {
                let Some(Value::Str(sep)) = args.pop() else {
                    return Err(self.fault("join needs a str separator"));
                };
                let mut parts = vec![];
                for it in &items {
                    match it {
                        Value::Str(s) => parts.push(s.clone()),
                        _ => return Err(self.fault("join needs []str")),
                    }
                }
                Ok(Value::Str(parts.join(&sep)))
            }
            _ => Err(self.fault(format!("list has no method {name}"))),
        }
    }

    fn str_method(&mut self, s: &str, name: &str, mut args: Vec<Value>) -> Result<Value, Fault> {
        let one_str = |this: &Self, args: &mut Vec<Value>| -> Result<String, Fault> {
            match args.pop() {
                Some(Value::Str(x)) => Ok(x),
                _ => Err(this.fault(format!("{name} needs a str argument"))),
            }
        };
        match name {
            "trim" => Ok(Value::Str(s.trim().to_string())),
            "upper" => Ok(Value::Str(s.to_uppercase())),
            "lower" => Ok(Value::Str(s.to_lowercase())),
            "split" => {
                let sep = one_str(self, &mut args)?;
                Ok(Value::List(
                    s.split(&sep).map(|p| Value::Str(p.to_string())).collect(),
                ))
            }
            "contains" => {
                let needle = one_str(self, &mut args)?;
                Ok(Value::Bool(s.contains(&needle)))
            }
            "starts_with" => {
                let p = one_str(self, &mut args)?;
                Ok(Value::Bool(s.starts_with(&p)))
            }
            "ends_with" => {
                let p = one_str(self, &mut args)?;
                Ok(Value::Bool(s.ends_with(&p)))
            }
            "replace" => {
                let to = one_str(self, &mut args)?;
                let from = one_str(self, &mut args)?;
                Ok(Value::Str(s.replace(&from, &to)))
            }
            "find" => {
                let needle = one_str(self, &mut args)?;
                Ok(match s.find(&needle) {
                    Some(byte) => Value::Int(s[..byte].chars().count() as i64),
                    None => Value::NoneV,
                })
            }
            "fields" => Ok(Value::List(
                s.split_whitespace()
                    .map(|p| Value::Str(p.to_string()))
                    .collect(),
            )),
            "lines" => Ok(Value::List(
                s.lines().map(|p| Value::Str(p.to_string())).collect(),
            )),
            "trim_prefix" => {
                let p = one_str(self, &mut args)?;
                Ok(Value::Str(s.strip_prefix(&p).unwrap_or(s).to_string()))
            }
            "trim_suffix" => {
                let p = one_str(self, &mut args)?;
                Ok(Value::Str(s.strip_suffix(&p).unwrap_or(s).to_string()))
            }
            "chars" => Ok(Value::List(
                s.chars().map(|c| Value::Str(c.to_string())).collect(),
            )),
            "repeat" => match args.pop() {
                Some(Value::Int(n)) if n >= 0 => {
                    // allocation failure aborts without unwinding, so the
                    // panic net cannot catch a silly size; fault first.
                    const MAX_REPEAT_BYTES: usize = 1 << 30;
                    match s.len().checked_mul(n as usize) {
                        Some(b) if b <= MAX_REPEAT_BYTES => Ok(Value::Str(s.repeat(n as usize))),
                        _ => Err(self.fault("repeat result too large")),
                    }
                }
                Some(Value::Int(_)) => Err(self.fault("repeat needs a nonnegative count")),
                _ => Err(self.fault("repeat needs an int argument")),
            },
            _ => Err(self.fault(format!("str has no method {name}"))),
        }
    }

    pub(crate) fn module_call(
        &mut self,
        module: &str,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, Fault> {
        let mangled = format!("{module}.{name}");
        if self.has_fn(&mangled) {
            return self.call_fn_by_name(&mangled, args);
        }
        let module = module.to_string();
        crate::stdlib::call(self, &module, name, args)
    }

    pub(crate) fn module_const(&mut self, module: &str, name: &str) -> Result<Value, Fault> {
        let module = module.to_string();
        crate::stdlib::constant(self, &module, name)
    }

    fn format(&self, fmt: &str, args: &[Value]) -> Result<String, Fault> {
        let pieces = crate::fmt::parse(fmt).map_err(|e| self.fault(e.msg("printf")))?;
        let mut out = String::new();
        let mut next = 0usize;
        for piece in pieces {
            let (width, prec, verb) = match piece {
                crate::fmt::Piece::Lit(s) => {
                    out.push_str(&s);
                    continue;
                }
                crate::fmt::Piece::Verb { width, prec, verb } => (width, prec, verb),
            };
            let arg = args
                .get(next)
                .ok_or_else(|| self.fault("printf: wrong argument count"))?;
            next += 1;
            let rendered = match (verb, arg) {
                ('v', v) => render(v),
                ('d', Value::Int(i)) => i.to_string(),
                ('s', Value::Str(s)) => s.clone(),
                ('t', Value::Bool(b)) => b.to_string(),
                ('q', Value::Str(s)) => format!("{s:?}"),
                ('f', Value::Float(f)) => match prec {
                    Some(p) => format!("{f:.p$}"),
                    None => format!("{f:.6}"),
                },
                (v, _) => {
                    return Err(self.fault(format!("printf: bad argument for %{v}")));
                }
            };
            // pad by character count, not byte length
            let n = rendered.chars().count();
            if n < width {
                out.push_str(&" ".repeat(width - n));
            }
            out.push_str(&rendered);
        }
        if next != args.len() {
            return Err(self.fault("printf: wrong argument count"));
        }
        Ok(out)
    }
}
