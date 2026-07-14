# byte and []byte Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `byte` scalar and compact `[]byte` list type per `docs/specs/2026-07-13-bytes-design.md`, including zero-copy always-view bridge crossing and chunked file handles.

**Architecture:** `byte` is a new nullary `Type`/`Value` scalar; `[]byte` is `Type::List(Box::new(Type::Byte))` at check time and a new compact `Value::Bytes(Rc<RefCell<BytesBuf>>)` at runtime. The bridge crosses `[]byte` as a buffer-protocol `#[pyclass]` view (the repo's first pyclass). `File` handles follow the `Proc` opaque-handle pattern exactly.

> **AMENDED 2026-07-14 (ADR 0022, during Task 3):** there is NO lent flag and NO in-place append growth — the sole-owner heuristic proved unsound (`c := append(b, x)` broke purity) and `append` on `[]byte` always copies. `BytesBuf` has only `data`. Buffer addresses are stable unconditionally, so Task 7 needs no growth guard and must NOT set or reference `lent`. Every mention of `lent`/growth in Tasks 3 and 7 below is superseded by this note; the append-after-lend detach golden in Task 7 remains valid (append always detaches).

**Tech Stack:** Rust (tree-walking interpreter), pyo3 0.29 (buffer protocol, `Python::attach`), golden-test harness (`tests/golden.rs`).

## Global Constraints

- The full gate is `NEVLA_TEST_PY=1 cargo test`. Green before every commit.
- Any change to language semantics MUST update `language-spec.md` in the same commit (repo CLAUDE.md). Each task below names its spec sections; they are part of that task's commit, not a docs pass at the end.
- `src/bridge.rs` is the only file that may name pyo3. `src/bridge_wasm.rs` must mirror any new public bridge surface with stubs.
- User programs may fault but must NEVER panic. No `unwrap`/`expect` reachable from user source.
- Do not move a type across the value/reference split (ADR 0010) — `byte` is a value type, `[]byte` a reference type, per the approved design.
- Commit style: terse, verb-first, area prefix (`check:`, `eval:`, `stdlib:`, `bridge:`, `spec:`). No AI attribution, no `Co-Authored-By: Claude` trailers, no dashes in messages.
- Golden tests are the executable spec: every language-visible behavior lands as a golden first (`.nv` + `.out`, or `.err` with one expected substring per line). `py/` cases only run under `NEVLA_TEST_PY=1`.
- Names are descriptive (ADR 0017): no new abbreviations.

**Read before starting any task:** `docs/specs/2026-07-13-bytes-design.md` (the approved design; normative for every behavioral question below).

---

### Task 1: The byte scalar — type, value, conversions, comparisons, ADR

**Files:**
- Modify: `src/types.rs` (Type enum ~:2-18, Display ~:20-62)
- Modify: `src/typecheck/mod.rs` (resolve ~:244-259, map-key allowlist ~:263)
- Modify: `src/typecheck/expr.rs` (Conv matrix ~:336-355, binary ~:365-460)
- Modify: `src/value.rs` (Value enum :24, MapKey :58-81, eq_value :134, render_depth :235)
- Modify: `src/interp.rs` (zero ~:1363-1371)
- Modify: `src/parser.rs` (CONV_NAMES :39)
- Modify: `src/builtins.rs` (convert ~:77-158)
- Modify: `src/bridge.rs` (to_py_depth :375-417, one arm)
- Modify: `language-spec.md` (5.1, 5.3 map keys, 5.11, 7.7, 7.9.2, 13.5 inbound table, 14.1 rendering note, plus the 17.x printf note: `%v` renders byte; `%d` requires `int(b)`)
- Create: `docs/adr/0021-byte-and-compact-byte-lists.md`
- Test: `tests/golden/eval/byte-scalar.nv` + `.out`, `tests/golden/eval/byte-conv-fault.nv` + `.err`, `tests/golden/check/byte-arithmetic.nv` + `.err`, `tests/golden/check/byte-int-mixing.nv` + `.err`

**Interfaces:**
- Produces: `Type::Byte` (nullary), `Value::Byte(u8)`, `MapKey::Byte(u8)`, `"byte"` in `CONV_NAMES`, `convert` handling `("byte", Value::Int)` with out-of-range fault message `byte conversion out of range: <n>`. Every later task relies on these exact names.

- [ ] **Step 1: Write the failing goldens**

`tests/golden/eval/byte-scalar.nv`:
```nevla
struct Header { kind byte }

fn main() {
    b := byte(7)
    print(int(b))            // 7
    print(b == byte(7))      // true
    print(byte(3) < byte(9)) // true
    h := Header{}
    print(h.kind)            // 0 (zero value)
    m := map[byte]str{byte(1): "one"}
    print(m[byte(1)])
    printf("%v\n", b)
}
```
`tests/golden/eval/byte-scalar.out`:
```
7
true
true
0
one
7
```
`tests/golden/eval/byte-conv-fault.nv`:
```nevla
fn main() {
    n := 300
    b := byte(n)
    print(int(b))
}
```
`tests/golden/eval/byte-conv-fault.err` (one substring per line):
```
byte conversion out of range: 300
```
`tests/golden/check/byte-arithmetic.nv`:
```nevla
fn main() {
    a := byte(1)
    b := byte(2)
    c := a + b
    print(int(c))
}
```
`tests/golden/check/byte-arithmetic.err`:
```
cannot apply operator
```
`tests/golden/check/byte-int-mixing.nv`:
```nevla
fn main() {
    n := 5
    b := byte(1)
    if b == n {
        print("no")
    }
}
```
`tests/golden/check/byte-int-mixing.err`:
```
byte
int
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --test golden`
Expected: FAIL — `byte-scalar.nv` errors with `unknown type byte` (resolve has no byte arm).

- [ ] **Step 3: Implement**

`src/types.rs` — add variant and display:
```rust
pub enum Type {
    Int,
    Byte,          // 0..=255 scalar; compare-only in v1 (design 2026-07-13)
    Float,
    // ... existing variants unchanged
}
// Display:
Type::Byte => write!(f, "byte"),
```
`src/typecheck/mod.rs` resolve: add `"byte" => Type::Byte,` to the scalar name match (~:248). Map-key allowlist (~:263): add `Type::Byte` to the `matches!`.

`src/value.rs`:
```rust
pub enum Value {
    Int(i64),
    /// byte scalar, 0..=255; value type like Int.
    Byte(u8),
    // ...
}
pub enum MapKey { Int(i64), Byte(u8), Str(String), Bool(bool) }
// from_value: Value::Byte(b) => Some(MapKey::Byte(*b)),
// to_value:   MapKey::Byte(b) => Value::Byte(*b),
// eq_value:   (Value::Byte(a), Value::Byte(b)) => Some(a == b),
// render_depth: Value::Byte(b) => b.to_string(),
```
`src/interp.rs` zero: `"byte" => Value::Byte(0),` in the scalar table (~:1366).

`src/parser.rs`: `const CONV_NAMES: &[&str] = &["int", "float", "str", "bool", "byte"];`

`src/typecheck/expr.rs` Conv matrix (~:336): allow, single-valued: `(Type::Byte, Type::Byte)`, `(Type::Byte, Type::Int)` (narrowing; fault is runtime), `(Type::Int, Type::Byte)` (widening). Do NOT add `(Type::Byte, Type::Str)` — `byte(s)` stays "cannot convert" per design.

`src/typecheck/expr.rs` binary: ordered comparisons (~:420-433) add `(Type::Byte, Type::Byte)`; equality (~:434-450) add `Byte` to the scalar `matches!` (`lt == rt` already enforces no byte/int mixing — the `byte-int-mixing` golden proves it).

`src/builtins.rs` convert (~:124-157):
```rust
("byte", Value::Byte(b)) => Ok(Value::Byte(b)),
("byte", Value::Int(n)) => {
    if !(0..=255).contains(&n) {
        return Err(self.fault(format!("byte conversion out of range: {n}")));
    }
    Ok(Value::Byte(n as u8))
}
("int", Value::Byte(b)) => Ok(Value::Int(b as i64)),
```
`src/bridge.rs` to_py_depth: `Value::Byte(b) => PyInt::new(py, *b as i64).into_any().unbind(),`

`docs/adr/0021-byte-and-compact-byte-lists.md` — Status/Context/Decision/Consequences per repo ADR shape. Records: byte scalar (value type, compare-only, no arithmetic — Go deviation, uint8 wraps), `[]byte` as ordinary list type with compact runtime representation (reference type; ADR 0010 placement), literal-assignability rule, `byte(n)` fault, `str(b)` fallibility (Go deviation: Go `string(b)` accepts invalid bytes, nevla str is characters per ADR 0019), always-view bridge crossing with the stable-address argument and lent flag, the buffer-family future (`[]float64`/`[]int64`), deferred lazy container proxies and zero-copy outbound. Content is a condensation of `docs/specs/2026-07-13-bytes-design.md`.

`language-spec.md`: 5.1 byte joins the scalar table (0..=255, compare-only, no arithmetic); 5.3 map key types gain byte; 5.11 zero value `byte(0)` rendered `0`; 7.7 conversion rows (`byte(x)`: byte identity, int narrows with runtime fault out of 0..=255, str not permitted; `int(x)` gains byte operand); 7.9.2 byte joins ordered comparisons; 13.5 inbound table row `byte → int`; 17.x printf: `%v` renders byte, `%d` needs `int(b)`.

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test`
Expected: PASS including the four new goldens. Note: `eq_value` and `render_depth` are exhaustive matches — the build fails until every arm is added, which is the intended checklist.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "check, eval: byte scalar; compare-only, byte(n) faults out of range (ADR 0021)"
```

---

### Task 2: Integer-literal assignability to byte

**Files:**
- Modify: `src/typecheck/expr.rs`, `src/typecheck/mod.rs` (every `accepts` call site that can see an expected `Type::Byte` with the argument `Expr` in hand)
- Modify: `language-spec.md` (5.10)
- Test: `tests/golden/eval/byte-literal.nv` + `.out`, `tests/golden/check/byte-literal-range.nv` + `.err`

**Interfaces:**
- Consumes: `Type::Byte`, `Value::Byte` from Task 1.
- Produces: `Checker::expects(&mut self, want: &Type, got: &Type, arg: Option<&Expr>) -> bool` — the single chokepoint later tasks (ListLit for `[]byte{...}`, method args) route through.

- [ ] **Step 1: Write the failing goldens**

`tests/golden/eval/byte-literal.nv`:
```nevla
struct Header { kind byte }

fn take(b byte) byte {
    return b
}

fn main() {
    b := byte(0)
    b = 200
    print(int(b))              // 200
    print(b == 200)            // true
    print(b < 255)             // true
    h := Header{kind: 3}
    print(int(h.kind))         // 3
    print(int(take(9)))        // 9
}
```
`tests/golden/eval/byte-literal.out`:
```
200
true
true
3
9
```
`tests/golden/check/byte-literal-range.nv`:
```nevla
fn main() {
    b := byte(0)
    b = 300
    print(int(b))
}
```
`tests/golden/check/byte-literal-range.err`:
```
cannot use 300 as byte
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test golden`
Expected: FAIL — `byte-literal.nv` reports assignment/argument type errors (`byte` vs `int`).

- [ ] **Step 3: Implement**

Add to `Checker` (in `expr.rs`):
```rust
/// accepts() plus the one byte implicit: an integer literal in 0..=255
/// is assignable to byte (design 2026-07-13); out of range is its own
/// diagnostic. Every checker site with the argument Expr in hand routes
/// through this instead of accepts().
pub(super) fn expects(&mut self, want: &Type, got: &Type, arg: Option<&Expr>, span: Span) -> bool {
    if *want == Type::Byte && *got == Type::Int {
        if let Some(e) = arg {
            if let ExprKind::Int(n) = e.kind {
                if (0..=255).contains(&n) {
                    return true;
                }
                self.diag(span, format!("cannot use {n} as byte (out of 0..255)"));
                return true; // diagnosed here; suppress the generic mismatch
            }
        }
    }
    want.accepts(got)
}
```
Route through it (grep `accepts(` under `src/typecheck/`; these are the sites with an `Expr` available): ListLit items (expr.rs ~:114), MapLit values, StructLit fields (~:176), `check_args` and `args_with_fn` (thread the `&[Expr]` they already have), assignment and short-var statements in `mod.rs`, index-assign RHS, equality/ordered comparison operands in `binary` (a `Byte` on one side and an in-range `Int` literal on the other passes; follow the `NoneLit` value-inspection precedent at expr.rs ~:435), and `append`'s element check (~:495). Negative literals arrive as unary minus, not `ExprKind::Int` — they fail with the ordinary mismatch, which is correct.

`language-spec.md` 5.10: the literal rule, verbatim from the design ("Integer literals in range are assignable to byte... the only implicit; a variable of type int never assigns to byte without conversion").

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test` — PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "check: in range integer literals assign to byte; the one implicit (spec 5.10)"
```

---

### Task 3: Value::Bytes — the compact buffer and every list operation

**Files:**
- Modify: `src/value.rs` (BytesBuf/BytesRef/Value::Bytes, eq_value, render_depth)
- Modify: `src/interp.rs` (index :1275, assign_into ~:781, slice :1309, list_range :610, ListLit eval, zero ~:1390)
- Modify: `src/builtins.rs` (len :26, append :32, clone :45, method_call :160, convert list-target naming ~:88)
- Modify: `src/typecheck/sigs.rs` (container_method: `sorted` comparable allowlist gains `Byte`)
- Modify: `language-spec.md` (5.2, 7.2.1, 7.5, 7.6, 8.7, 11.1 note, 14)
- Test: `tests/golden/builtins/bytes-core.nv` + `.out`, `tests/golden/eval/bytes-index-fault.nv` + `.err`, `tests/golden/eval/bytes-alias.nv` + `.out`

**Interfaces:**
- Consumes: `Type::Byte`, `Value::Byte`, `expects` from Tasks 1-2.
- Produces:
```rust
// src/value.rs
pub struct BytesBuf {
    pub data: Vec<u8>,
    /// set on first bridge crossing; a lent buffer never grows in place
    /// (stable addresses; design 2026-07-13). Never cleared.
    pub lent: bool,
}
pub type BytesRef = Rc<RefCell<BytesBuf>>;
// Value::Bytes(BytesRef); constructor Value::bytes(data: Vec<u8>) -> Value
```
Tasks 4-8 rely on `Value::bytes(...)` and the `lent` field by these exact names.

- [ ] **Step 1: Write the failing goldens**

`tests/golden/builtins/bytes-core.nv`:
```nevla
fn main() {
    b := []byte{137, 80, 78, 71}
    print(len(b))            // 4
    print(int(b[0]))         // 137
    b[0] = 1
    print(int(b[0]))         // 1
    b = append(b, 255)
    print(len(b))            // 5
    s := b[1:3]
    print(s)                 // [80, 78]
    total := 0
    for _, x := range b {
        total = total + int(x)
    }
    print(total)             // 1 + 80 + 78 + 71 + 255 = 485
    print(b.contains(byte(78)))  // true
    print([]byte{3, 1, 2}.sorted())  // [1, 2, 3]
    var z []byte
    print(len(z))            // 0
    print(b)                 // [1, 80, 78, 71, 255]
}
```
`tests/golden/builtins/bytes-core.out`:
```
4
137
1
5
[80, 78]
485
true
[1, 2, 3]
0
[1, 80, 78, 71, 255]
```
(Adjust the `var z []byte` line to the repo's actual zero-declaration syntax — check an existing golden under `tests/golden/eval/` for the idiom; if there is no var form, use a struct field of type `[]byte` as in Task 1.)

`tests/golden/eval/bytes-index-fault.nv`:
```nevla
fn main() {
    b := []byte{1, 2}
    print(int(b[5]))
}
```
`tests/golden/eval/bytes-index-fault.err` — copy the exact out-of-range fault wording from the existing list case (see `tests/golden/eval/` index fault goldens), e.g.:
```
index 5 out of range
```
`tests/golden/eval/bytes-alias.nv`:
```nevla
fn main() {
    a := []byte{1, 2, 3}
    b := a
    b[0] = 9
    print(int(a[0]))         // 9: reference type, aliasing
    c := clone(a)
    c[1] = 7
    print(int(a[1]))         // 2: clone detaches
}
```
`tests/golden/eval/bytes-alias.out`:
```
9
2
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test golden`
Expected: FAIL — `[]byte{...}` evaluates to a boxed `Value::List`; `int(b[0])` fails because the element is `Value::Int`, and the compact behaviors are absent. (If some pass by accident via the generic list path, that's fine — the implementation still switches representation and all must pass after.)

- [ ] **Step 3: Implement**

`src/value.rs`: `BytesBuf`/`BytesRef`/`Value::Bytes` as in Interfaces; `Value::bytes(data)` helper mirroring `Value::list`; `eq_value` arm `(Value::Bytes(a), Value::Bytes(b)) => Some(a.borrow().data == b.borrow().data)` (checker forbids `==` on `[]byte`, but `contains` and struct compare reach this); `render_depth` renders `[1, 2, 3]` exactly like List.

`src/interp.rs`:
- Index read: `(Value::Bytes(b), Value::Int(i))` — bounds-fault with the same message format as List, else `Value::Byte(b.borrow().data[i as usize])`.
- `assign_into` Bytes arm: accept RHS `Value::Byte(x)` or `Value::Int(x)` (the literal rule types `b[i] = 255` as Int at runtime); write in place, never realloc.
- Slice: copy the sub-range into a fresh `Value::bytes(...)`, same bounds rule as List.
- `list_range` iteration: bind `Value::Byte` elements; length fixes at entry like List (snapshot the len, read per index).
- ListLit eval: when the literal's element `TypeExpr` is `Named("byte")`, build `Value::bytes` from the item values (each `Value::Int` in 0..=255 — checker-guaranteed — or `Value::Byte`).
- `zero`: `TypeExpr::List(inner)` where inner is `Named("byte")` → `Value::bytes(vec![])`.

`src/builtins.rs`:
- `len`: `Value::Bytes(b) => Value::Int(b.borrow().data.len() as i64)`.
- `append`: pure with the growth rule:
```rust
Some(Value::Bytes(buf)) => {
    let elem = /* second arg: Value::Byte(x) => x, Value::Int(n) => n as u8 (checker-guaranteed range) */;
    // sole owner and never lent: grow in place, return the same Rc.
    // Observationally identical to copy+rebind; O(1) amortized for builds.
    if Rc::strong_count(&buf) == 1 && !buf.borrow().lent {
        buf.borrow_mut().data.push(elem);
        Ok(Value::Bytes(buf))
    } else {
        let mut data = buf.borrow().data.clone();
        data.push(elem);
        Ok(Value::bytes(data))
    }
}
```
(Note: `strong_count == 1` requires the arg to be moved in, not cloned from the environment slot — the binding being rebound still holds a reference during the call, so in practice the count is 2 for `b = append(b, x)`. Check how list append receives its arg; if the receiver reference is always live, use `strong_count <= 2` with a comment, or keep it simple: always-copy in this task and revisit with a bench note in the ADR. Correctness first; the golden asserts behavior, not allocation.)
- `clone`: fresh `BytesBuf { data: ..., lent: false }`.
- `method_call`: `Value::Bytes(buf) => self.bytes_method(buf, name, args)` — materialize `Vec<Value::Byte>`, delegate to `list_method`, and repack `filter`/`sorted` results into `Value::bytes` (they are typed `[]byte` by the checker); `map` stays a boxed List (its element type changes). All delegated methods are pure, so the snapshot is correct.
- `convert` target naming (~:88): `TypeExpr::List(inner)` inspects inner for `Named("byte")` → internal key `"bytes"`; `("bytes", Value::Bytes(b)) => Ok(Value::Bytes(b))` identity.

`src/typecheck/sigs.rs` container_method: `sorted` allowlist becomes `Int | Byte | Float | Str | Unknown`.

`language-spec.md`: 7.2.1 (`[]byte{...}` composite literals, the literal rule applies per element); 7.5 (indexing `[]byte` yields `byte`); 7.6 (slicing yields `[]byte`, copies); 8.7 (iteration binds `byte`); 11.1 (one sentence: `[]byte` is a list type, reference kind, compact representation is an implementation note); 14 (`len`/`append`/`clone` over `[]byte`; `sorted`/`contains`/`filter`/`each`/`map` apply; `sum`/`join` do not).

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test` — PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "eval: compact []byte value; index, slice, append, clone, iteration, methods (spec 7.5, 7.6, 8.7, 14)"
```

---

### Task 4: []byte(s) encode and str(b) decode

**Files:**
- Modify: `src/typecheck/expr.rs` (Conv ok-matrix ~:336, fallible-matrix ~:350)
- Modify: `src/builtins.rs` (convert)
- Modify: `language-spec.md` (7.7)
- Test: `tests/golden/eval/bytes-str-roundtrip.nv` + `.out`, `tests/golden/eval/bytes-str-invalid.nv` + `.out`, `tests/golden/check/str-bytes-unconsumed.nv` + `.err`

**Interfaces:**
- Consumes: `Value::bytes`, the `"bytes"` convert key from Task 3.
- Produces: `[]byte(s)` single-valued; `str(b)` multi-valued `(str, error?)` with error message containing `invalid UTF-8`.

- [ ] **Step 1: Write the failing goldens**

`tests/golden/eval/bytes-str-roundtrip.nv`:
```nevla
fn main() {
    b := []byte("héllo")
    print(len(b))            // 6: UTF-8 bytes, not characters
    s, err := str(b)
    if err != none {
        print("unexpected")
        return
    }
    print(s)                 // héllo
    print(len(s))            // 5: characters (ADR 0019)
}
```
`tests/golden/eval/bytes-str-roundtrip.out`:
```
6
héllo
5
```
`tests/golden/eval/bytes-str-invalid.nv`:
```nevla
fn main() {
    b := []byte{255, 254}
    s, err := str(b)
    if err != none {
        print("invalid")
        print(len(s))        // 0: zero value in the value slot
        return
    }
    print("decoded")
}
```
`tests/golden/eval/bytes-str-invalid.out`:
```
invalid
0
```
`tests/golden/check/str-bytes-unconsumed.nv`:
```nevla
fn main() {
    b := []byte{104, 105}
    print(str(b))
}
```
`tests/golden/check/str-bytes-unconsumed.err` — the repo's standard unconsumed-multi-value diagnostic; copy the exact wording from an existing golden that misuses `int("x")` inline (look in `tests/golden/check/`), e.g.:
```
must be consumed
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test golden`
Expected: FAIL — `[]byte("héllo")` is a compile error today ("cannot convert"), `str(b)` is single-valued.

- [ ] **Step 3: Implement**

`src/typecheck/expr.rs` Conv: ok-matrix adds `(List(Byte), Str)` single-valued; fallible-matrix adds `(Str, List(Byte))` — the first non-py fallible `str(x)`; keep every other `str(x)` single-valued.

`src/builtins.rs` convert:
```rust
("bytes", Value::Str(s)) => Ok(Value::bytes(s.into_bytes())),
("str", Value::Bytes(b)) => {
    let data = b.borrow().data.clone();
    Ok(match String::from_utf8(data) {
        Ok(s) => Value::Tuple(vec![Value::Str(s), Value::NoneV]),
        Err(_) => Value::Tuple(vec![
            Value::Str(String::new()),
            Value::Err(ErrVal { msg: "invalid UTF-8 in byte conversion to str".into(), ..Default::default() }),
        ]),
    })
}
```
(Match the existing fallible-parse pattern — see how `("int", Value::Str)` builds its tuple, and mirror exactly.)

`language-spec.md` 7.7: `[]byte(s)` row (UTF-8 encode, never fails); `str(x)` row gains the `[]byte` operand exception (decode, fallible) with a sentence flagging it as the sole non-py fallible source; conversion table note that `[]byte(x)` on a nevla `[]byte` is identity (the existing `[]T(x)` pass-through rule).

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test` — PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "check, eval: []byte(s) encodes, str(b) decodes fallibly; the one non py fallible str (spec 7.7)"
```

---

### Task 5: file.readbytes and file.writebytes

**Files:**
- Modify: `src/typecheck/sigs.rs` (~:41-47, file rows)
- Modify: `src/stdlib/file.rs` (call arms; unit tests in the existing `#[cfg(test)] mod tests`)
- Modify: `language-spec.md` (15.3)
- Test: `tests/golden/stdlib/file-bytes.nv` + `.out`; Rust unit tests in `file.rs`

**Interfaces:**
- Consumes: `Value::bytes`, `Type::Byte`.
- Produces: `file.readbytes(path str) ([]byte, error?)`, `file.writebytes(path str, b []byte) error?`.

- [ ] **Step 1: Write the failing tests**

`tests/golden/stdlib/file-bytes.nv` (self-contained `/tmp` case, cleanup included, per `file-roundtrip.nv` convention):
```nevla
import "file"

fn main() {
    p := "/tmp/nevla-golden-file-bytes"
    b := []byte{0, 255, 137, 10}          // deliberately not valid UTF-8
    err := file.writebytes(p, b)
    if err != none {
        print("write failed")
        return
    }
    got, rerr := file.readbytes(p)
    if rerr != none {
        print("read failed")
        return
    }
    print(len(got))          // 4
    print(int(got[1]))       // 255
    _, derr := str(got)
    print(derr != none)      // true: content is not UTF-8, and that is fine
    _ = file.remove(p)
    missing, merr := file.readbytes("/nonexistent/nevla-bytes")
    print(len(missing))      // 0: empty []byte on error
    print(merr != none)      // true
}
```
`tests/golden/stdlib/file-bytes.out`:
```
4
255
true
0
true
```
Rust unit test in `file.rs` `mod tests` (tempbase pattern, alongside the existing ones):
```rust
#[test]
fn readbytes_roundtrips_non_utf8() {
    let base = tempbase("bytes");
    let p = base.join("x.bin");
    std::fs::write(&p, [0u8, 255, 137]).unwrap();
    let v = call_read_helper(&p); // drive file::call(("readbytes", ...)) the way existing tests drive read
    // assert the tuple is (Bytes([0,255,137]), NoneV)
}
```
(Shape it exactly like the neighboring tests in `file.rs:148-217` — same helpers, same assertion style.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test` — FAIL: checker rejects `file.readbytes` (no sig).

- [ ] **Step 3: Implement**

`src/typecheck/sigs.rs`:
```rust
("file", "readbytes") => Member::Fn(vec![Str], vec![List(Box::new(Byte)), err_opt()]),
("file", "writebytes") => Member::Fn(vec![Str, List(Box::new(Byte))], vec![err_opt()]),
```
`src/stdlib/file.rs` call arms (mirror `read`/`write` at :29-38):
```rust
("readbytes", [Value::Str(path)]) => Ok(fallible(
    std::fs::read(path).map(Value::bytes).map_err(|e| format!("readbytes {path}: {e}")),
    Value::bytes(vec![]),
)),
("writebytes", [Value::Str(path), Value::Bytes(b)]) => {
    Ok(match std::fs::write(path, &b.borrow().data) {
        Ok(()) => Value::NoneV,
        Err(e) => err(format!("writebytes {path}: {e}")),
    })
}
```
(Adopt the exact error-message format of the neighboring `read`/`write` arms — inspect and match, don't invent.)

`language-spec.md` 15.3: replace "there is no bytes type in v1" with the binary pair, signatures and the empty-`[]byte`-on-error rule, per the design.

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test` — PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "stdlib: file.readbytes and file.writebytes; binary file io (spec 15.3)"
```

---

### Task 6: File handles — open, create, read(n), write, close

**Files:**
- Modify: `src/value.rs` (Value::File variant + render/eq arms)
- Modify: `src/stdlib/file.rs` (FileInner, struct_types(), call arms, method(), unit tests)
- Modify: `src/typecheck/mod.rs` (~:121-123, inject file's struct types like proc's)
- Modify: `src/typecheck/sigs.rs` (open/create rows)
- Modify: `src/typecheck/expr.rs` (~:146-148 construction block; ~:813-838 method arm)
- Modify: `src/builtins.rs` (method_call arm ~:170)
- Modify: `src/imports.rs` (injected_struct_owners ~:22-61)
- Modify: `language-spec.md` (15.3)
- Test: `tests/golden/stdlib/file-handle.nv` + `.out`, `tests/golden/stdlib/file-handle-closed.nv` + `.out`

**Interfaces:**
- Consumes: `Value::bytes`, sigs `Byte`.
- Produces: `Value::File(std::sync::Arc<crate::stdlib::file::FileInner>)`; checker type `Struct("File")`; methods `read(n int) ([]byte, error?)`, `write(b []byte) error?`, `close() error?`; module fns `file.open(path str) (File, error?)`, `file.create(path str) (File, error?)`. EOF is `(empty []byte, none)`.

- [ ] **Step 1: Write the failing goldens**

`tests/golden/stdlib/file-handle.nv`:
```nevla
import "file"

fn main() (error?) {
    p := "/tmp/nevla-golden-file-handle"
    w := check file.create(p)
    check w.write([]byte{1, 2, 3, 4, 5, 6, 7})
    check w.close()

    r := check file.open(p)
    total := 0
    for {
        chunk, err := r.read(3)
        if err != none {
            return err
        }
        if len(chunk) == 0 {
            break
        }
        total = total + len(chunk)
    }
    check r.close()
    print(total)             // 7: chunked read to EOF (3 + 3 + 1)
    _ = file.remove(p)
    return none
}
```
(`for {}`/`break` per spec 8.7/8.8; `check` per 7.8 — if `check` cannot apply to `error?`-only returns in statement position, use the explicit `if err != none` form as in the chunk loop.)
`tests/golden/stdlib/file-handle.out`:
```
7
```
`tests/golden/stdlib/file-handle-closed.nv`:
```nevla
import "file"

fn main() {
    p := "/tmp/nevla-golden-file-closed"
    w, err := file.create(p)
    if err != none {
        print("create failed")
        return
    }
    _ = w.close()
    _ = w.close()                    // close is idempotent: no error
    werr := w.write([]byte{1})
    print(werr != none)              // true: write after close is an error value
    _ = file.remove(p)
}
```
`tests/golden/stdlib/file-handle-closed.out`:
```
true
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test golden` — FAIL: `file.open` unknown.

- [ ] **Step 3: Implement** (the five handle touch-points, Proc as the literal template)

`src/stdlib/file.rs`:
```rust
/// Open file handle; reference type behind Arc, like Proc.
/// Mutex<Option<..>>: None after close, making close idempotent and
/// post-close ops ordinary error values.
pub struct FileInner {
    pub path: String,
    handle: std::sync::Mutex<Option<std::fs::File>>,
}

pub fn struct_types() -> Vec<(String, Vec<(String, crate::types::Type)>)> {
    vec![("File".into(), vec![])]      // zero fields: not constructible
}

pub fn method(interp: &Interp, f: &FileInner, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    match (name, args.as_slice()) {
        ("read", [Value::Int(n)]) => { /* take up to n via Read::take on the guarded handle;
            closed => (bytes(vec![]), err); Ok(k bytes) => (bytes, NoneV); EOF is k == 0 => (empty, NoneV) */ }
        ("write", [Value::Bytes(b)]) => { /* write_all; closed or io error => err value; else NoneV */ }
        ("close", []) => { /* guard.take(); always Ok(Value::NoneV) — idempotent */ }
        _ => Err(interp.fault(format!("File has no method {name}"))),
    }
}
```
Follow `proc.rs::method` (:565-582) for the exact `Value::Tuple` fallible-return construction and `proc.rs` (:36-50) for struct_types. `read` must not fault on a closed handle — closed is an error value, per the golden.

- `src/value.rs`: `File(std::sync::Arc<crate::stdlib::file::FileInner>)` + render arm (render as `<File /path>` — match Proc's rendering style, check `render_depth`'s Proc arm) + `eq_value` arm (identity via `Arc::ptr_eq`, mirroring Proc's arm).
- `src/typecheck/mod.rs` :121-123: inject `file::struct_types()` alongside proc's.
- `src/typecheck/expr.rs` :146-148: extend the not-constructible block: `"File"` → "File cannot be constructed; use file.open or file.create".
- `src/typecheck/expr.rs` :813-838 method arm:
```rust
Type::Struct(s) if s == "File" => match name {
    "read"  => { self.check_args(&[Int], args, span); ExprTy::Multi(vec![List(Box::new(Byte)), err_opt()]) }
    "write" => { self.check_args(&[List(Box::new(Byte))], args, span); ExprTy::One(err_opt()) }
    "close" => { self.check_args(&[], args, span); ExprTy::One(err_opt()) }
    _ => { self.diag(span, format!("File has no method {name}")); ExprTy::One(Unknown) }
}
```
- `src/typecheck/sigs.rs`: `("file","open") | ("file","create") => Member::Fn(vec![Str], vec![Struct("File".into()), err_opt()])`.
- `src/builtins.rs` method_call: `Value::File(f) => crate::stdlib::file::method(self, &f, name, args),`.
- `src/imports.rs` injected_struct_owners: add file → File.
- `file.rs` call arms for `open`/`create` returning `Value::Tuple(vec![Value::File(...), Value::NoneV])` or the error tuple.
- Unit tests in `file.rs` (tempbase): read-to-EOF chunk loop; write-after-close returns error value; double-close ok.

`language-spec.md` 15.3: the `File` handle section verbatim from the design (open/create/read/write/close, EOF rule, idempotent close, no seek/append-handle in v1). Note `File` in the copy-model listing if 11.1 enumerates reference types (it joins Proc/Ctx as an opaque handle).

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test` — PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "stdlib: File handles; open, create, chunked read, write, idempotent close (spec 15.3)"
```

---

### Task 7: Bridge inbound — the buffer-protocol view

**Files:**
- Modify: `src/bridge.rs` (first `#[pyclass]`; to_py_depth arm; unit tests)
- Modify: `language-spec.md` (13.5: the per-kind bridge model paragraph + `[]byte → buffer view` inbound row)
- Test: Rust `#[test]`s in `bridge.rs`; `tests/golden/py/bytes.nv` + `.out`

**Interfaces:**
- Consumes: `Value::Bytes`, `BytesRef`, the `lent` flag.
- Produces: `[]byte` arguments reach Python as a buffer-protocol object (class name `nevla.bytesview`); passing sets `lent = true` permanently.

- [ ] **Step 1: Write the failing tests**

Rust test in `bridge.rs` (follow `matmul_dispatches_to_python` :509+ — `init(None)` then `Python::attach`):
```rust
#[test]
fn bytes_cross_as_zero_copy_view() {
    init(None).unwrap();
    let buf = crate::value::Value::bytes(vec![1, 2, 3]);
    // pass to python len(): expect 3
    // build memoryview over it py-side and hold it;
    // write data[0] = 9 through the BytesRef;
    // assert the held memoryview sees 9 (shared memory, not a copy)
    // assert the buffer's lent flag is now true
}
```
`tests/golden/py/bytes.nv`:
```nevla
import py "hashlib"

fn main() {
    b := []byte("hello world")
    whole := check hashlib.sha256(b)
    print(str(check whole.hexdigest()))

    chunked := check hashlib.sha256()
    check chunked.update(b[0:5])
    check chunked.update(b[5:11])
    print(str(check chunked.hexdigest()))    // same digest: chunked == whole
}
```
`tests/golden/py/bytes.out`:
```
b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
```
(Adjust the `str(check ...)`/`check` composition to the py-chain consumption rules of spec 7.11/13 — copy the call shape from `tests/golden/py/convert.nv`.)

- [ ] **Step 2: Run to verify failure**

Run: `NEVLA_TEST_PY=1 cargo test` — FAIL: `[]byte` hits to_py_depth's `other =>` arm: "cannot pass [...] to python".

- [ ] **Step 3: Implement**

In `src/bridge.rs` (native only; nothing needed in `bridge_wasm.rs` — its `to_py_handle` stub already errors for every value):
```rust
/// nevla []byte crossing the bridge: a buffer-protocol view over the
/// buffer's memory. Zero-copy by design (design 2026-07-13): stable
/// addresses are guaranteed because no nevla operation reallocates a
/// live buffer once lent. unsendable: the runtime is single-threaded
/// and BytesRef is an Rc.
#[pyclass(unsendable, name = "bytesview", module = "nevla")]
struct BytesView {
    buf: crate::value::BytesRef,
}

#[pymethods]
impl BytesView {
    unsafe fn __getbuffer__(
        slf: pyo3::PyRef<'_, Self>,
        view: *mut pyo3::ffi::Py_buffer,
        flags: std::os::raw::c_int,
    ) -> pyo3::PyResult<()> {
        // fill ptr/len from slf.buf.borrow().data (itemsize 1, format "B",
        // one dim, writable); keep a clone of the Rc alive via the pyclass
        // instance itself (obj field of Py_buffer -> slf). Use
        // pyo3::ffi::PyBuffer_FillInfo for the boring parts.
        // The borrow ends here; the pointer stays valid because lent
        // buffers never grow in place (append copies) and writes are
        // element-wise. Single-threaded + GIL: no torn access.
    }
    unsafe fn __releasebuffer__(slf: pyo3::PyRef<'_, Self>, view: *mut pyo3::ffi::Py_buffer) {
        // nothing to free; the Rc drops with the pyclass instance.
    }
}
```
to_py_depth arm:
```rust
Value::Bytes(buf) => {
    buf.borrow_mut().lent = true;    // permanent: this buffer never grows in place again
    Py::new(py, BytesView { buf: buf.clone() })?.into_any()
}
```
(Map the `PyErr` into the file's `errval` idiom like the neighbors.) Verify Task 3's append growth rule now observes `lent` — the py/ golden below proves detach.

Add to `tests/golden/py/bytes.nv` (same file, after the digests):
```nevla
    held := check memoryview(b)          // if bare builtins aren't reachable, use py "builtins" import
    b[0] = 72
    print(int(check int(held[0])))       // 72: the view shares memory
    b2 := append(b, 33)
    b2[0] = 90
    print(int(check int(held[0])))       // 72 still: append copied (lent), the view kept the old buffer
```
with `.out` gaining `72` and `72`. (Exact py-builtins access: follow how other py/ goldens reach builtins — e.g. `import py "builtins"` then `builtins.memoryview(b)`.)

`language-spec.md` 13.5: insert the per-kind data model paragraph (values convert; contiguous primitive buffers cross by reference via the buffer protocol; containers copy; py handles are references — condensed from the design) and the inbound row: `[]byte` → buffer view (`nevla.bytesview`), zero-copy, mutations visible both ways, `bytes(x)` py-side materializes a copy when a true `bytes` is required.

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test` — PASS, including the memoryview visibility and append-detach assertions.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "bridge: []byte crosses as a zero copy buffer view; lent buffers never grow in place (spec 13.5)"
```

---

### Task 8: Bridge outbound — []byte(x) extraction from Python buffers

**Files:**
- Modify: `src/bridge.rs` (ConvTarget :421-431, extract :443-466)
- Modify: `src/bridge_wasm.rs` (mirror ConvTarget :82-89)
- Modify: `src/builtins.rs` (py-source convert path ~:94-121: map `[]byte` target → `ConvTarget::Bytes`)
- Modify: `src/typecheck/expr.rs` (confirm `(List(Byte), Py)` falls in the fallible py-source rule; add if the matrix is explicit)
- Modify: `language-spec.md` (13.5 outbound table row)
- Test: extend `tests/golden/py/bytes.nv` (or a sibling `py/bytes-extract.nv`) + `.out`/`.err`

**Interfaces:**
- Consumes: `Value::bytes`, `BytesView` from Task 7.
- Produces: `ConvTarget::Bytes`; `[]byte(x)` on a py operand yields `([]byte, error?)`, extracting from anything exposing the buffer protocol (`bytes`, `bytearray`, `memoryview`), one copy in; non-buffer objects yield an error value.

- [ ] **Step 1: Write the failing golden**

`tests/golden/py/bytes-extract.nv`:
```nevla
import py "builtins"

fn main() {
    raw := check builtins.bytes([]byte{104, 105})     // round trip through a real python bytes
    got, err := []byte(raw)
    if err != none {
        print("extract failed")
        return
    }
    print(len(got))          // 2
    print(int(got[0]))       // 104

    ba := check builtins.bytearray(got)
    got2, err2 := []byte(ba)
    print(err2 == none)      // true: bytearray extracts
    print(len(got2))         // 2

    obj := check builtins.object()
    _, err3 := []byte(obj)
    print(err3 != none)      // true: non buffer object is an error value
}
```
`tests/golden/py/bytes-extract.out`:
```
2
104
true
2
true
```

- [ ] **Step 2: Run to verify failure**

Run: `NEVLA_TEST_PY=1 cargo test --test golden` — FAIL: `[]byte(raw)` unsupported extraction target.

- [ ] **Step 3: Implement**

`src/bridge.rs`: add `ConvTarget::Bytes` (`:421-431`); in `extract` (`:443`):
```rust
ConvTarget::Bytes => Python::attach(|py| {
    let bound = h.0.bind(py);
    match pyo3::buffer::PyBuffer::<u8>::get(bound) {
        Ok(buffer) => Ok(Value::bytes(buffer.to_vec(py).map_err(|e| errval(py, e))?)),
        Err(e) => Err(errval(py, e)),
    }
})
```
(Return-shape: match how the existing `ConvTarget::List` extraction reports errors — error value, not fault, flowing into the `(zero, error)` tuple built by the caller.)

`src/bridge_wasm.rs`: add `Bytes` to the mirror `ConvTarget` enum.

`src/builtins.rs` py-source path: the `TypeExpr → ConvTarget` mapping sends `List(Named("byte"))` to `ConvTarget::Bytes` instead of `List(Elem::...)`.

`src/typecheck/expr.rs`: py-source conversions are already uniformly fallible; verify `[]byte(pyval)` types as `([]byte, error?)` and add the pair only if the matrix enumerates targets explicitly.

`language-spec.md` 13.5 outbound table: `[]byte(x)` row — succeeds when the object supports the buffer protocol (`bytes`, `bytearray`, `memoryview`); one copy in; failure is an error value.

- [ ] **Step 4: Run gate**

Run: `NEVLA_TEST_PY=1 cargo test` — PASS.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "bridge: []byte(x) extracts from python buffers; bytes, bytearray, memoryview (spec 13.5)"
```

---

### Task 9: Closeout — backlog, self-review, full verification

**Files:**
- Modify: `docs/backlog.md` (the byte/[]byte entry: DESIGN APPROVED → DONE with date, matching the `proc: DONE` style)
- No other planned changes; this task is verification.

- [ ] **Step 1: Self-review the whole diff** (house rule): `git diff <commit-before-task-1>..HEAD` — hunt duplicated literals/logic (the byte range check must exist in exactly one runtime place: `convert`; the checker literal rule in exactly one place: `expects`) and misplaced functionality (nothing pyo3 outside `bridge.rs`; no `unwrap`/`expect` reachable from user source — grep the new code).

- [ ] **Step 2: Spec coverage check**: walk `docs/specs/2026-07-13-bytes-design.md` section by section; confirm each landed or is explicitly out-of-scope there. Confirm every semantics commit included its `language-spec.md` diff (`git log --stat` — each task commit shows the spec file).

- [ ] **Step 3: Run the full gate plus the packaging compile gate**

Run: `NEVLA_TEST_PY=1 cargo test`
Expected: PASS, zero skips outside `PENDING`.
Run: `cargo run -- fmt --check .` (or the repo's `nevla fmt --check` invocation per the Makefile) — formatter round-trips the new goldens.

- [ ] **Step 4: Update backlog and commit**

```bash
git add docs/backlog.md && git commit -m "docs: byte and []byte DONE in backlog"
```

---

## Self-review notes (already applied)

- Spec coverage: design sections map to tasks — shape/semantics (1-3), conversions (4), file (5-6), bridge model + view (7), outbound (8), out-of-scope items need no task, testing distributed per task, ADR in task 1, concurrency ledger lines already committed (`8a488d3`).
- Type consistency: `Type::Byte`, `Value::Byte(u8)`, `Value::Bytes(BytesRef)`, `BytesBuf{data, lent}`, `Value::bytes(...)`, `MapKey::Byte`, `ConvTarget::Bytes`, `Checker::expects` — names used identically across tasks.
- Known judgment calls the executor may hit: exact `var`/zero-declaration syntax in goldens (check existing goldens; adjust the case, keep the assertion); `strong_count` subtlety in append growth (Task 3 permits always-copy fallback — behavior over allocation); py-builtins access shape in py/ goldens (copy from existing py/ cases). These are noted inline at the point of use.
