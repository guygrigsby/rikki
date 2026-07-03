//! The only module that names pyo3. The evaluator sees `PyHandle` values and
//! operations returning rikki values or rikki error values; Python
//! exceptions never cross this boundary as anything but `ErrVal`.

use std::path::Path;
use std::sync::{Arc, Once};

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

use crate::ast::BinOp;
use crate::value::{ErrVal, Value};

/// Arc so rikki's deep-copy Clone never needs the GIL; py values are
/// shared references by design.
#[derive(Debug, Clone)]
pub struct PyHandle(Arc<Py<PyAny>>);

impl PyHandle {
    fn new(obj: Py<PyAny>) -> Self {
        PyHandle(Arc::new(obj))
    }
}

static INIT: Once = Once::new();

/// Initialize the embedded CPython. `venv` points at a project venv whose
/// site-packages should be importable; None means bare interpreter.
pub fn init(venv: Option<&Path>) {
    INIT.call_once(|| {
        if let Some(v) = venv {
            // CPython honors VIRTUAL_ENV only via activation scripts; the
            // reliable embed knob is PYTHONPATH to the venv's site-packages.
            let mut paths = vec![];
            let lib = v.join("lib");
            if let Ok(rd) = std::fs::read_dir(&lib) {
                for e in rd.flatten() {
                    let sp = e.path().join("site-packages");
                    if sp.is_dir() {
                        paths.push(sp.to_string_lossy().to_string());
                    }
                }
            }
            if !paths.is_empty() {
                let existing = std::env::var("PYTHONPATH").unwrap_or_default();
                let joined = if existing.is_empty() {
                    paths.join(":")
                } else {
                    format!("{}:{existing}", paths.join(":"))
                };
                // edition 2024 makes set_var unsafe (concurrent getenv is
                // UB on POSIX); safe today because init runs before the
                // interp and ctrlc threads read the environment. On the
                // edition bump, pass the path via PyConfig instead.
                std::env::set_var("PYTHONPATH", joined);
            }
        }
        Python::initialize();
        // Inside an embedded interpreter sys.executable is the host binary
        // (tk); libraries that re-exec `sys.executable -c ...`
        // (multiprocessing, joblib, tokenizers) would invoke the runner.
        // Point it at the venv's real python instead.
        if let Some(v) = venv {
            let py_bin = v.join("bin").join("python");
            if py_bin.exists() {
                let path = py_bin.to_string_lossy().to_string();
                Python::attach(|py| {
                    if let Ok(sys) = py.import("sys") {
                        let _ = sys.setattr("executable", &path);
                        let _ = sys.setattr("_base_executable", &path);
                    }
                });
            }
        }
    });
}

fn errval(py: Python<'_>, e: PyErr) -> ErrVal {
    let pytype = e
        .get_type(py)
        .name()
        .map(|n| n.to_string())
        .unwrap_or_else(|_| "Exception".into());
    let traceback = e
        .traceback(py)
        .and_then(|t| t.format().ok())
        .unwrap_or_default();
    ErrVal {
        msg: format!("{pytype}: {}", e.value(py)),
        cause: None,
        pytype,
        traceback,
    }
}

/// The zero value of type `py`: a handle to Python's None. Operations on it
/// raise proper Python errors instead of faulting the interpreter.
pub fn py_none() -> PyHandle {
    init(None);
    Python::attach(|py| PyHandle::new(py.None()))
}

/// "major.minor" of the linked CPython, readable before initialization.
pub fn embedded_python() -> String {
    let raw = unsafe { std::ffi::CStr::from_ptr(pyo3::ffi::Py_GetVersion()) };
    let s = raw.to_string_lossy();
    let ver = s.split_whitespace().next().unwrap_or("");
    let mut it = ver.split('.');
    match (it.next(), it.next()) {
        (Some(maj), Some(min)) => format!("{maj}.{min}"),
        _ => ver.to_string(),
    }
}

pub fn import(name: &str) -> Result<PyHandle, ErrVal> {
    init(None);
    Python::attach(|py| match py.import(name) {
        Ok(m) => Ok(PyHandle::new(m.into_any().unbind())),
        Err(e) => Err(errval(py, e)),
    })
}

pub fn getattr(h: &PyHandle, name: &str) -> Result<Value, ErrVal> {
    Python::attach(|py| {
        h.0.bind(py)
            .getattr(name)
            .map(|v| Value::Py(PyHandle::new(v.unbind())))
            .map_err(|e| errval(py, e))
    })
}

pub fn call(h: &PyHandle, args: &[Value], kwargs: &[(String, Value)]) -> Result<Value, ErrVal> {
    Python::attach(|py| {
        let converted: Result<Vec<Py<PyAny>>, ErrVal> = args.iter().map(|a| to_py(py, a)).collect();
        let tuple = PyTuple::new(py, converted?).map_err(|e| errval(py, e))?;
        let bound = h.0.bind(py);
        let result = if kwargs.is_empty() {
            bound.call1(tuple)
        } else {
            let d = PyDict::new(py);
            for (k, v) in kwargs {
                let val = to_py(py, v)?;
                d.set_item(k, val).map_err(|e| errval(py, e))?;
            }
            bound.call(tuple, Some(&d))
        };
        result
            .map(|v| Value::Py(PyHandle::new(v.unbind())))
            .map_err(|e| errval(py, e))
    })
}

/// A fresh python iterator over the object (`iter(obj)`).
pub fn iter(h: &PyHandle) -> Result<PyHandle, ErrVal> {
    Python::attach(|py| {
        h.0.bind(py)
            .try_iter()
            .map(|it| PyHandle::new(it.into_any().unbind()))
            .map_err(|e| errval(py, e))
    })
}

/// `__next__`: Some(item), None on StopIteration, Err on any other raise.
pub fn next(h: &PyHandle) -> Result<Option<Value>, ErrVal> {
    Python::attach(|py| match h.0.bind(py).call_method0("__next__") {
        Ok(v) => Ok(Some(Value::Py(PyHandle::new(v.unbind())))),
        Err(e) if e.is_instance_of::<pyo3::exceptions::PyStopIteration>(py) => Ok(None),
        Err(e) => Err(errval(py, e)),
    })
}

pub fn setattr(h: &PyHandle, name: &str, v: &Value) -> Result<(), ErrVal> {
    Python::attach(|py| {
        let val = to_py(py, v)?;
        h.0.bind(py).setattr(name, val).map_err(|e| errval(py, e))
    })
}

pub fn setitem(h: &PyHandle, key: &Value, v: &Value) -> Result<(), ErrVal> {
    Python::attach(|py| {
        let k = to_py(py, key)?;
        let val = to_py(py, v)?;
        h.0.bind(py).set_item(k, val).map_err(|e| errval(py, e))
    })
}

pub fn index(h: &PyHandle, idx: &Value) -> Result<Value, ErrVal> {
    Python::attach(|py| {
        let key = to_py(py, idx)?;
        h.0.bind(py)
            .get_item(key)
            .map(|v| Value::Py(PyHandle::new(v.unbind())))
            .map_err(|e| errval(py, e))
    })
}

pub fn binop(op: BinOp, l: &Value, r: &Value) -> Result<Value, ErrVal> {
    use pyo3::basic::CompareOp;
    Python::attach(|py| {
        let a = to_py(py, l)?;
        let b = to_py(py, r)?;
        let a = a.bind(py);
        let res = match op {
            BinOp::Add => a.add(b),
            BinOp::Sub => a.sub(b),
            BinOp::Mul => a.mul(b),
            BinOp::Div => a.div(b),
            BinOp::Rem => a.rem(b),
            BinOp::MatMul => a.matmul(b),
            BinOp::Eq => a.rich_compare(b, CompareOp::Eq),
            BinOp::NotEq => a.rich_compare(b, CompareOp::Ne),
            BinOp::Lt => a.rich_compare(b, CompareOp::Lt),
            BinOp::LtEq => a.rich_compare(b, CompareOp::Le),
            BinOp::Gt => a.rich_compare(b, CompareOp::Gt),
            BinOp::GtEq => a.rich_compare(b, CompareOp::Ge),
            BinOp::And | BinOp::Or => {
                return Err(ErrVal {
                    msg: "&& and || need bool, not py".into(),
                    ..Default::default()
                })
            }
        };
        res.map(|v| Value::Py(PyHandle::new(v.unbind())))
            .map_err(|e| errval(py, e))
    })
}

/// rikki → Python for arguments and indexes.
fn to_py(py: Python<'_>, v: &Value) -> Result<Py<PyAny>, ErrVal> {
    let obj: Py<PyAny> = match v {
        Value::Int(i) => PyInt::new(py, *i).into_any().unbind(),
        Value::Float(f) => PyFloat::new(py, *f).into_any().unbind(),
        Value::Bool(b) => PyBool::new(py, *b).to_owned().into_any().unbind(),
        Value::Str(s) => PyString::new(py, s).into_any().unbind(),
        Value::NoneV => py.None(),
        Value::Py(h) => (*h.0).clone_ref(py),
        Value::List(items) => {
            let converted: Result<Vec<Py<PyAny>>, ErrVal> =
                items.iter().map(|x| to_py(py, x)).collect();
            PyList::new(py, converted?)
                .map_err(|e| errval(py, e))?
                .into_any()
                .unbind()
        }
        Value::Map(m) => {
            let d = PyDict::new(py);
            for (k, val) in m {
                let key = to_py(py, &k.to_value())?;
                let value = to_py(py, val)?;
                d.set_item(key, value).map_err(|e| errval(py, e))?;
            }
            d.into_any().unbind()
        }
        other => {
            return Err(ErrVal {
                msg: format!("cannot pass {} to python", crate::value::render(other)),
                ..Default::default()
            })
        }
    };
    Ok(obj)
}

/// Python → rikki extraction target (spec 13.5), typed so the caller cannot
/// hand the bridge a spec it parses differently.
pub enum ConvTarget {
    Int,
    Float,
    Bool,
    Str,
    /// `[]T(x)`: elements extracted per `Elem`.
    List(Elem),
    /// A target extraction can never satisfy (an unknown named type, py,
    /// ...); always fails with an error value naming it.
    Other(String),
}

/// Element target of a list extraction; `Py` keeps elements as handles.
pub enum Elem {
    Int,
    Float,
    Bool,
    Str,
    Py,
}

/// Python → rikki extraction for the fallible conversions.
pub fn extract(target: &ConvTarget, h: &PyHandle) -> Result<Value, ErrVal> {
    Python::attach(|py| {
        let b = h.0.bind(py);
        match target {
            ConvTarget::Int => scalar(py, &Elem::Int, b),
            ConvTarget::Float => scalar(py, &Elem::Float, b),
            ConvTarget::Bool => scalar(py, &Elem::Bool, b),
            ConvTarget::Str => scalar(py, &Elem::Str, b),
            ConvTarget::List(elem) => {
                let mut out = vec![];
                let iter = b.try_iter().map_err(|e| errval(py, e))?;
                for item in iter {
                    let item = item.map_err(|e| errval(py, e))?;
                    out.push(scalar(py, elem, &item)?);
                }
                Ok(Value::List(out))
            }
            ConvTarget::Other(name) => Err(ErrVal {
                msg: format!("cannot convert py to {name}"),
                ..Default::default()
            }),
        }
    })
}

fn scalar(py: Python<'_>, kind: &Elem, b: &Bound<'_, PyAny>) -> Result<Value, ErrVal> {
    match kind {
        Elem::Int => b
            .extract::<i64>()
            .map(Value::Int)
            .map_err(|e| errval(py, e)),
        Elem::Float => b
            .extract::<f64>()
            .map(Value::Float)
            .map_err(|e| errval(py, e)),
        Elem::Bool => b.is_truthy().map(Value::Bool).map_err(|e| errval(py, e)),
        Elem::Str => b
            .str()
            .map(|s| Value::Str(s.to_string_lossy().to_string()))
            .map_err(|e| errval(py, e)),
        Elem::Py => Ok(Value::Py(PyHandle::new(b.clone().unbind()))),
    }
}

/// Rendering for print/%v: Python str() of the object.
pub fn display(h: &PyHandle) -> String {
    Python::attach(|py| {
        h.0.bind(py)
            .str()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|_| "<py>".into())
    })
}

/// Is `name` a Python standard-library module? Used by the manifest check:
/// stdlib imports need no declaration in rikki.toml.
pub fn is_stdlib(name: &str) -> bool {
    init(None);
    Python::attach(|py| {
        py.import("sys")
            .and_then(|sys| sys.getattr("stdlib_module_names"))
            .and_then(|names| names.contains(name))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    // the test venv has no numpy/torch, so the runtime matmul proof lives
    // here: a python class defining __matmul__ in the embedded interpreter
    #[test]
    fn matmul_dispatches_to_python() {
        init(None);
        let h = Python::attach(|py| {
            let ns = pyo3::types::PyDict::new(py);
            py.run(
                std::ffi::CString::new(
                    "class M:\n    def __matmul__(self, other):\n        return 42",
                )
                .unwrap()
                .as_c_str(),
                None,
                Some(&ns),
            )
            .unwrap();
            let m = ns.get_item("M").unwrap().unwrap().call0().unwrap();
            PyHandle::new(m.unbind())
        });
        let out = binop(crate::ast::BinOp::MatMul, &Value::Py(h.clone()), &Value::Py(h)).unwrap();
        let Value::Py(r) = out else { panic!("{out:?}") };
        assert_eq!(display(&r), "42");
    }

    #[test]
    fn embedded_python_is_major_minor() {
        let v = super::embedded_python();
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 2, "{v}");
        assert!(
            parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())),
            "{v}"
        );
    }
}
