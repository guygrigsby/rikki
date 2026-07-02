use indexmap::IndexMap;
use std::rc::Rc;

use crate::ast::{Block, Param, TypeExpr};

/// Runtime value. `Clone` is a deep copy (value semantics); `py` and ctx
/// values are the documented reference exceptions, and closures share their
/// immutable captured environment (ADR 0009).
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    List(Vec<Value>),
    /// Insertion order is language-visible (iteration, rendering); the
    /// IndexMap is semantics, not an optimization target.
    Map(IndexMap<MapKey, Value>),
    /// Field order is declaration order, also language-visible.
    Struct {
        name: String,
        fields: IndexMap<String, Value>,
    },
    /// The absent option value. Present option values are stored bare;
    /// the typechecker is what keeps them honest.
    NoneV,
    /// Live Python object; reference semantics, the documented exception.
    Py(crate::bridge::PyHandle),
    Err(ErrVal),
    Fn(FnRef),
    Module(String),
    /// Opaque context: deadline plus interrupt flag. Shared by reference,
    /// like py values; it is a handle, not data.
    Ctx(std::sync::Arc<crate::stdlib::ctx::CtxInner>),
    Tuple(Vec<Value>),
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    Int(i64),
    Str(String),
    Bool(bool),
}

impl MapKey {
    pub fn from_value(v: &Value) -> Option<MapKey> {
        match v {
            Value::Int(i) => Some(MapKey::Int(*i)),
            Value::Str(s) => Some(MapKey::Str(s.clone())),
            Value::Bool(b) => Some(MapKey::Bool(*b)),
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            MapKey::Int(i) => Value::Int(*i),
            MapKey::Str(s) => Value::Str(s.clone()),
            MapKey::Bool(b) => Value::Bool(*b),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ErrVal {
    pub msg: String,
    pub cause: Option<Box<ErrVal>>,
    pub pytype: String,
    pub traceback: String,
}

#[derive(Debug, Clone)]
pub enum FnRef {
    /// Top-level function, by name.
    Decl(String),
    /// Shared, not deep-copied: ClosureData is immutable after capture, so
    /// sharing is observationally value-like (ADR 0009).
    Closure(Rc<ClosureData>),
    /// Zero value of a fn type; calling it is a fault.
    Zero,
}

#[derive(Debug)]
pub struct ClosureData {
    pub params: Vec<Param>,
    /// Declared return types, for zero-filling `check` early returns.
    pub ret: Vec<TypeExpr>,
    pub body: Block,
    /// Captured-by-value snapshot of the visible scope, flattened.
    pub captured: std::collections::HashMap<String, Value>,
}

impl Value {
    /// Equality per the language: scalars, none, options, and structural
    /// (recursive) equality for lists, structs, maps, and tuples. Py, fn,
    /// ctx, module, error, and unit values never compare equal, even to
    /// themselves. Matching on `self` exhaustively (no catch-all) so a new
    /// variant forces an equality decision at compile time.
    pub fn eq_value(&self, other: &Value) -> bool {
        match self {
            Value::Int(a) => matches!(other, Value::Int(b) if a == b),
            Value::Float(a) => matches!(other, Value::Float(b) if a == b),
            Value::Bool(a) => matches!(other, Value::Bool(b) if a == b),
            Value::Str(a) => matches!(other, Value::Str(b) if a == b),
            Value::NoneV => matches!(other, Value::NoneV),
            Value::List(a) => match other {
                Value::List(b) => eq_seq(a, b),
                _ => false,
            },
            Value::Tuple(a) => match other {
                Value::Tuple(b) => eq_seq(a, b),
                _ => false,
            },
            Value::Struct {
                name: an,
                fields: af,
            } => match other {
                Value::Struct {
                    name: bn,
                    fields: bf,
                } => {
                    an == bn
                        && af.len() == bf.len()
                        && af
                            .iter()
                            .all(|(k, v)| bf.get(k).is_some_and(|w| v.eq_value(w)))
                }
                _ => false,
            },
            Value::Map(a) => match other {
                Value::Map(b) => {
                    a.len() == b.len()
                        && a.iter()
                            .all(|(k, v)| b.get(k).is_some_and(|w| v.eq_value(w)))
                }
                _ => false,
            },
            Value::Py(_)
            | Value::Err(_)
            | Value::Fn(_)
            | Value::Module(_)
            | Value::Ctx(_)
            | Value::Unit => false,
        }
    }
}

fn eq_seq(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_value(y))
}

/// Canonical rendering, shared by print, %v, str() conversion, and the
/// bridge's "cannot pass X to python" diagnostics.
pub fn render(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.clone(),
        Value::List(items) => {
            let inner = items.iter().map(render).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        Value::Map(m) => {
            let inner = m
                .iter()
                .map(|(k, v)| format!("{}: {}", render(&k.to_value()), render(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
        Value::Struct { name, fields } => {
            let inner = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", render(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}{{{inner}}}")
        }
        Value::NoneV => "none".into(),
        Value::Py(h) => crate::bridge::display(h),
        Value::Err(e) => format!("error({})", e.msg),
        Value::Fn(_) => "fn".into(),
        Value::Module(m) => format!("module {m}"),
        Value::Ctx(_) => "ctx".into(),
        Value::Tuple(items) => items.iter().map(render).collect::<Vec<_>>().join(", "),
        Value::Unit => "()".into(),
    }
}
