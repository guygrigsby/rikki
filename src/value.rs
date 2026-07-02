use indexmap::IndexMap;
use std::rc::Rc;

use crate::ast::{Block, Param, TypeExpr};

/// Runtime value. `Clone` is a deep copy (value semantics); the future `py`
/// variant is the documented reference exception.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    List(Vec<Value>),
    Map(IndexMap<MapKey, Value>),
    Struct { name: String, fields: IndexMap<String, Value> },
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
    pub fn is_none(&self) -> bool {
        matches!(self, Value::NoneV)
    }

    /// Equality per the language: scalars, none, options, and structural
    /// (recursive) equality for lists, structs, maps, and tuples. Py, fn,
    /// ctx, and module values never compare equal.
    pub fn eq_value(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::NoneV, Value::NoneV) => true,
            (Value::NoneV, _) | (_, Value::NoneV) => false,
            (Value::List(a), Value::List(b)) | (Value::Tuple(a), Value::Tuple(b)) => {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(x, y)| x.eq_value(y))
            }
            (
                Value::Struct { name: an, fields: af },
                Value::Struct { name: bn, fields: bf },
            ) => {
                an == bn
                    && af.len() == bf.len()
                    && af.iter().all(|(k, v)| {
                        bf.get(k).is_some_and(|w| v.eq_value(w))
                    })
            }
            (Value::Map(a), Value::Map(b)) => {
                a.len() == b.len()
                    && a.iter().all(|(k, v)| {
                        b.get(k).is_some_and(|w| v.eq_value(w))
                    })
            }
            _ => false,
        }
    }
}
