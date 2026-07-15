//! The bridge's wasm stand-in: same public surface as bridge.rs, no
//! python. Every operation returns a clean error value (or faults where
//! the native bridge would), so playground programs that touch py get a
//! reported error, never a crash. Checked programs without py never call
//! into here except for zero values and display.

use std::path::Path;

use crate::ast::BinOp;
use crate::value::{ErrVal, Value};

/// A handle to nothing: the py zero value renders as None and every
/// operation on it reports unavailability.
#[derive(Debug, Clone)]
pub struct PyHandle;

fn unavailable() -> ErrVal {
    ErrVal {
        msg: "python is not available in this build".into(),
        ..Default::default()
    }
}

pub fn init(_venv: Option<&Path>) {}

pub fn py_none() -> PyHandle {
    PyHandle
}

pub fn embedded_python() -> String {
    "unavailable".into()
}

pub fn import(_name: &str) -> Result<PyHandle, ErrVal> {
    Err(unavailable())
}

pub fn getattr(_h: &PyHandle, _name: &str) -> Result<Value, ErrVal> {
    Err(unavailable())
}

pub fn call(_h: &PyHandle, _args: &[Value], _kwargs: &[(String, Value)]) -> Result<Value, ErrVal> {
    Err(unavailable())
}

pub fn iter(_h: &PyHandle) -> Result<PyHandle, ErrVal> {
    Err(unavailable())
}

pub fn next(_h: &PyHandle) -> Result<Option<Value>, ErrVal> {
    Err(unavailable())
}

pub fn setattr(_h: &PyHandle, _name: &str, _v: &Value) -> Result<(), ErrVal> {
    Err(unavailable())
}

pub fn setitem(_h: &PyHandle, _key: &Value, _v: &Value) -> Result<(), ErrVal> {
    Err(unavailable())
}

pub fn index(_h: &PyHandle, _idx: &Value) -> Result<Value, ErrVal> {
    Err(unavailable())
}

pub fn binop(_op: BinOp, _l: &Value, _r: &Value) -> Result<Value, ErrVal> {
    Err(unavailable())
}

pub fn enter(_h: &PyHandle) -> Result<(), ErrVal> {
    Err(unavailable())
}

pub fn exit(_h: &PyHandle, _err: Option<&ErrVal>) -> Result<bool, ErrVal> {
    Err(unavailable())
}

pub fn to_py_handle(_v: &Value) -> Result<PyHandle, ErrVal> {
    Err(unavailable())
}

pub enum ConvTarget {
    Int,
    Float,
    Bool,
    Str,
    List(Elem),
    Bytes,
    Other(String),
}

pub enum Elem {
    Int,
    Float,
    Bool,
    Str,
    Py,
}

pub fn extract(_target: &ConvTarget, _h: &PyHandle) -> Result<Value, ErrVal> {
    Err(unavailable())
}

pub fn display(_h: &PyHandle) -> String {
    "None".into()
}

pub fn is_stdlib(_name: &str) -> bool {
    false
}

/// No Python, nothing deferred: nothing to flush.
pub fn release_pending() {}
