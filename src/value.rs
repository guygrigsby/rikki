use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::ast::{Block, Param, TypeExpr};

/// Shared list storage: lists are reference types (ADR 0010).
pub type ListRef = Rc<RefCell<Vec<Value>>>;
/// Shared map storage: maps are reference types (ADR 0010).
pub type MapRef = Rc<RefCell<IndexMap<MapKey, Value>>>;

/// Compact `[]byte` storage: a contiguous `Vec<u8>` instead of a boxed
/// `Vec<Value>` (design 2026-07-13). Reference type, like `List`.
#[derive(Debug)]
pub struct BytesBuf {
    pub data: Vec<u8>,
}
pub type BytesRef = Rc<RefCell<BytesBuf>>;

/// Recursion budget for deep walks over values (render, structural
/// compare, bridge conversion). Aliasing makes cyclic values
/// constructible; the cap turns a would-be hang into a fault or a
/// truncated rendering.
pub const DEPTH_LIMIT: u32 = 256;

/// Runtime value, split Go's way (ADR 0010): scalars, strings, structs,
/// tuples, and errors are value types (`Clone` copies them); lists, maps,
/// fn, py, and ctx are reference types (`Clone` copies the reference).
/// A struct copy is shallow in Go's sense: reference-typed fields alias.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    /// byte scalar, 0..=255; value type like Int.
    Byte(u8),
    Float(f64),
    Bool(bool),
    Str(String),
    /// Reference type: assignment, argument passing, and capture alias
    /// the one underlying list. Insertion order is language-visible.
    List(ListRef),
    /// Reference type: the compact `[]byte` buffer (design 2026-07-13).
    /// Every list rule applies (aliasing, index-assign, pure append, clone
    /// one-level, slices copy); the runtime storage is a plain `Vec<u8>`
    /// instead of a boxed `Vec<Value>`.
    Bytes(BytesRef),
    /// Reference type, like List. Insertion order is language-visible.
    Map(MapRef),
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
    /// A nevla stdlib module or file-import namespace, by name. Python
    /// modules are not these; they are `Py` handles.
    Module(String),
    /// Opaque context: deadline plus interrupt flag. Shared by reference,
    /// like py values; it is a handle, not data.
    Ctx(std::sync::Arc<crate::stdlib::ctx::CtxInner>),
    Re(std::sync::Arc<regex::Regex>),
    Proc(std::sync::Arc<crate::stdlib::proc::ProcInner>),
    /// Open file handle; reference type behind Arc, like Proc (spec 15.3).
    File(std::sync::Arc<crate::stdlib::file::FileInner>),
    Tuple(Vec<Value>),
    Unit,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MapKey {
    /// Also holds `byte` keys: a bare-literal key evaluates to `Value::Int`
    /// at runtime (spec 5.10) while a `byte(n)`-converted key evaluates to
    /// `Value::Byte`, so folding both into one variant here is what makes
    /// `map[byte]{...}` see them as the same key regardless of which
    /// spelling wrote it or which spelling reads it back. The checker
    /// guarantees only byte-typed keys reach a `map[byte]`, so this can
    /// never collide with a genuine `int` key. `keys()`/iteration over a
    /// `map[byte]` therefore yields `Value::Int`-tagged keys; this is the
    /// established mixed-tag representation (`eq_value` and `binop`
    /// already compare Byte/Int numerically, so it is observationally
    /// transparent — see `tests/golden/builtins/bytes-mixed-tags.nv`).
    Int(i64),
    Str(String),
    Bool(bool),
}

impl MapKey {
    pub fn from_value(v: &Value) -> Option<MapKey> {
        match v {
            Value::Int(i) => Some(MapKey::Int(*i)),
            Value::Byte(b) => Some(MapKey::Int(*b as i64)),
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
    /// "file:line" (or "line N") where this error was born: error.new,
    /// error.wrap, a test helper, or a py exception crossing the bridge
    /// (spec 10.1). Empty when unknown.
    pub origin: String,
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
    /// The cells of the literal's free variables (ADR 0010): shared with
    /// the enclosing scope, so reads and writes flow both ways.
    pub captured: std::collections::HashMap<String, Rc<RefCell<Value>>>,
}

impl Value {
    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(RefCell::new(items)))
    }

    /// Fresh compact byte buffer.
    pub fn bytes(data: Vec<u8>) -> Value {
        Value::Bytes(Rc::new(RefCell::new(BytesBuf { data })))
    }

    /// Extract a `[]byte` element from a checker-approved slot: either the
    /// canonical `Value::Byte`, or `Value::Int` from the literal rule (spec
    /// 5.10) — a bare int literal in byte position always evaluates as Int,
    /// since `eval()` has no expected-type context to coerce against. Out
    /// of range is only reachable from the unchecked repl; `None` there,
    /// never a truncating cast.
    pub fn as_byte_elem(&self) -> Option<u8> {
        match self {
            Value::Byte(b) => Some(*b),
            Value::Int(n) => u8::try_from(*n).ok(),
            _ => None,
        }
    }

    pub fn map(m: IndexMap<MapKey, Value>) -> Value {
        Value::Map(Rc::new(RefCell::new(m)))
    }

    /// Equality per the language: scalars, none, options, and structural
    /// (recursive) equality for lists, structs, maps, and tuples. Py, fn,
    /// ctx, module, error, and unit values never compare equal, even to
    /// themselves. None means the walk exceeded DEPTH_LIMIT (a cyclic or
    /// absurdly deep value); callers fault. Matching on `self`
    /// exhaustively (no catch-all) so a new variant forces an equality
    /// decision at compile time.
    pub fn eq_value(&self, other: &Value, depth: u32) -> Option<bool> {
        if depth > DEPTH_LIMIT {
            return None;
        }
        Some(match self {
            // Byte and Int cross-compare numerically: the checker admits a
            // (Byte, Int) pair wherever an in-range int literal is
            // assignable to a byte-typed slot (spec 5.10), and unlike a
            // local's static type, a literal always evaluates to
            // Value::Int regardless of which side of the comparison it's
            // on. binop's (Eq, Byte, Int) arm (interp.rs) already widens
            // the same way for `==`; this is the same rule for eq_value's
            // callers (list contains, struct/list-of-struct equality, and
            // map *value* comparison during map equality), which don't go
            // through binop at all. Map *key* matching never reaches this
            // function at all — it goes through MapKey's derived
            // PartialEq/Hash, which is why `MapKey::from_value` folds
            // Value::Byte into MapKey::Int rather than relying on this
            // widening.
            Value::Int(a) => match other {
                Value::Int(b) => a == b,
                Value::Byte(b) => *a == *b as i64,
                _ => false,
            },
            Value::Byte(a) => match other {
                Value::Byte(b) => a == b,
                Value::Int(b) => *a as i64 == *b,
                _ => false,
            },
            Value::Float(a) => matches!(other, Value::Float(b) if a == b),
            Value::Bool(a) => matches!(other, Value::Bool(b) if a == b),
            Value::Str(a) => matches!(other, Value::Str(b) if a == b),
            Value::NoneV => matches!(other, Value::NoneV),
            Value::List(a) => match other {
                Value::List(b) => Rc::ptr_eq(a, b) || eq_seq(&a.borrow(), &b.borrow(), depth + 1)?,
                _ => false,
            },
            // The checker forbids `==` on []byte (design 2026-07-13), but
            // `contains` and struct/list-of-struct equality still reach
            // this arm through eq_value directly.
            Value::Bytes(a) => match other {
                Value::Bytes(b) => Rc::ptr_eq(a, b) || a.borrow().data == b.borrow().data,
                _ => false,
            },
            Value::Tuple(a) => match other {
                Value::Tuple(b) => eq_seq(a, b, depth + 1)?,
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
                    if an != bn || af.len() != bf.len() {
                        false
                    } else {
                        for (k, v) in af {
                            match bf.get(k) {
                                Some(w) => {
                                    if !v.eq_value(w, depth + 1)? {
                                        return Some(false);
                                    }
                                }
                                None => return Some(false),
                            }
                        }
                        true
                    }
                }
                _ => false,
            },
            Value::Map(a) => match other {
                Value::Map(b) => {
                    if Rc::ptr_eq(a, b) {
                        true
                    } else {
                        let (a, b) = (a.borrow(), b.borrow());
                        if a.len() != b.len() {
                            false
                        } else {
                            for (k, v) in a.iter() {
                                match b.get(k) {
                                    Some(w) => {
                                        if !v.eq_value(w, depth + 1)? {
                                            return Some(false);
                                        }
                                    }
                                    None => return Some(false),
                                }
                            }
                            true
                        }
                    }
                }
                _ => false,
            },
            Value::Py(_)
            | Value::Err(_)
            | Value::Fn(_)
            | Value::Module(_)
            | Value::Ctx(_)
            | Value::Re(_)
            | Value::Proc(_)
            | Value::File(_)
            | Value::Unit => false,
        })
    }
}

fn eq_seq(a: &[Value], b: &[Value], depth: u32) -> Option<bool> {
    if a.len() != b.len() {
        return Some(false);
    }
    for (x, y) in a.iter().zip(b) {
        if !x.eq_value(y, depth)? {
            return Some(false);
        }
    }
    Some(true)
}

/// Canonical rendering, shared by print, %v, str() conversion, and the
/// bridge's "cannot pass X to python" diagnostics. Rendering is for eyes:
/// past DEPTH_LIMIT (cyclic values) it truncates to "..." rather than
/// faulting.
pub fn render(v: &Value) -> String {
    render_depth(v, 0)
}

fn render_depth(v: &Value, depth: u32) -> String {
    if depth > DEPTH_LIMIT {
        return "...".into();
    }
    let r = |x: &Value| render_depth(x, depth + 1);
    match v {
        Value::Int(i) => i.to_string(),
        Value::Byte(b) => b.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => s.clone(),
        Value::List(items) => {
            let inner = items.borrow().iter().map(r).collect::<Vec<_>>().join(", ");
            format!("[{inner}]")
        }
        Value::Bytes(b) => {
            let inner = b
                .borrow()
                .data
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{inner}]")
        }
        Value::Map(m) => {
            let inner = m
                .borrow()
                .iter()
                .map(|(k, v)| format!("{}: {}", r(&k.to_value()), r(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{inner}}}")
        }
        Value::Struct { name, fields } => {
            let inner = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", r(v)))
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
        Value::Re(re) => format!("re({})", re.as_str()),
        Value::Proc(_) => "proc".into(),
        Value::File(f) => format!("<File {}>", f.path),
        Value::Tuple(items) => items.iter().map(r).collect::<Vec<_>>().join(", "),
        Value::Unit => "()".into(),
    }
}
