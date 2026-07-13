# byte and []byte: binary data as a compact list

Date: 2026-07-13. Status: approved. Source: backlog v1.1 top item (binary
file and http bodies); ADR 0019 promised bytes its own record.

## Shape

Two additions, both ordinary members of existing kinds:

- `byte` is a predeclared scalar value type holding 0 through 255, zero
  value `0`. Legal everywhere a type is legal: struct fields, parameters,
  returns, map keys.
- `[]byte` is the list type over it. Every list rule of spec chapter 11
  applies verbatim: reference type, aliasing on assignment and capture,
  index-assign, pure `append`, `clone` one-level, slices copy, iteration
  binds elements, not a map key, no `==`.

There is no new sequence kind and no `bytes` type. The design deliberately
rejected two earlier shapes: an immutable value `bytes` (Python's) loses
in-place mutation and forces a second type the day a buffer needs editing;
an element type legal only inside `[]` breaks the compositional type
grammar. A real scalar plus the existing list machinery is the whole
language cost.

## byte semantics

- `b[i]` yields `byte`; `b[i] = x` takes `byte`. The `[]T`-indexes-to-`T`
  rule holds unchanged.
- Integer literals in range are assignable to `byte` at compile time:
  `[]byte{137, 80}`, `b[i] = 255`, `x == 137` all work bare. An
  out-of-range literal in byte position is a compile error. This is the
  only implicit; a variable of type `int` never assigns to `byte` without
  conversion.
- Comparisons: `==`, `!=`, `<`, `<=`, `>`, `>=` between bytes (and against
  in-range integer literals via the rule above). byte is a valid map key.
- No arithmetic operators in v1. Math means widening first:
  `n := int(b[0])*256 + int(b[1])`. This avoids the wrap-vs-fault question
  and byte/int mixing rules entirely; adding arithmetic later is
  compatible. Deviation from Go (uint8 wraps, full arithmetic) recorded in
  the ADR.
- Conversions (spec 7.7): `int(b)` widens, single-valued. `byte(n)` from
  `int` narrows and faults when `n` is outside 0..255; the range rule
  lives here and nowhere else. Go truncates silently; nevla does not do
  silent. Bounds-check first for data-driven narrowing. `byte(s)` from
  `str` is not permitted in v1 ("cannot convert"); parse with `int(s)`
  and narrow.

## []byte conversions

- `[]byte(s)` from `str`: UTF-8 encode, single-valued, never fails. The
  conversion grammar's `SliceType "(" Expression ")"` slot already parses
  this.
- `str(b)` from `[]byte`: UTF-8 decode, fallible: `s, err := str(b)`.
  nevla strings are character sequences (ADR 0019), so invalid UTF-8
  cannot pass. This makes `[]byte` the first non-`py` operand for which
  `str(x)` can fail; the spec's "str(x) never fails" row gains the
  exception, the ADR records the Go deviation (Go's `string(b)` accepts
  any bytes).
- Composite literals come free from list syntax: `[]byte{137, 80, 78, 71}`.
- Rendering: `print` and `%v` render like a list of its elements,
  consistent with lists.

## Runtime representation

Invisible in the spec, and the reason the design works: `[]byte` gets a
compact sibling of `Value::List` backed by `Rc<RefCell<Vec<u8>>>`. A 10MB
body is 10MB contiguous, not ten million boxed `Value`s. The typechecker
knows the static type; the interpreter dispatches list operations on the
compact representation. A bare `byte` value is `Value::Byte(u8)`.

## Bridge (spec 13.5)

- Inbound: `byte` converts to Python `int`; `[]byte` converts to Python
  `bytes` as one contiguous copy. This is the cheapest row in the inbound
  table; nevla lists already copy per-element with a fresh PyObject each
  (`to_py_depth`), so one memcpy is a strict improvement on the existing
  posture, not a new cost.
- Outbound: `[]byte(x)` on a `py` operand extracts from `bytes`,
  `bytearray`, and `memoryview` (buffer protocol), one copy in, fallible
  like every outbound conversion. `byte(x)` on a `py` operand: deferred;
  `int(x)` then `byte(n)` covers it.
- Large data does not transit the bridge as a value, by idiom: shards and
  tensors load py-side by path (`torch.load(path)`,
  `safetensors.safe_open`, `np.memmap`) and live as `py` handles. `[]byte`
  serves the small and medium band: http payloads, images into
  `io.BytesIO`, headers, checksums, download-then-load-by-path.
- Zero-copy (a Python memoryview over nevla's buffer, Rc pinned) is
  explicitly deferred: Python holding a view into a mutable nevla buffer
  is an aliasing design of its own. Re-entry: the ADR names it; a line
  lands in docs/proposals/concurrency.md, since a lent buffer and a shared
  buffer are the same problem.

## stdlib

`file` (spec 15.3) grows the binary pair:

- `file.readbytes(path str) ([]byte, error?)` — whole-file read; empty
  `[]byte` on error.
- `file.writebytes(path str, b []byte) error?` — create or truncate.

No `appendbytes` until something wants it.

## Out of scope, with re-entry paths

- Binary http. `Request`/`Response` declare `body str` as struct types;
  flipping to `[]byte` breaks every http program and taxes the text
  majority, parallel fields need their own thought. Break-early philosophy
  means the flip stays available. Backlog keeps the line.
- Hex integer literals (`0x89`). Lexer-only, orthogonal; backlog item if
  decimal grates.
- `[]int(b)` / `[]byte(xs)` cross-conversions: a `for` loop covers it.
- byte arithmetic and bit operators: see above; compatible later.
- Zero-copy bridge views: see above.

## Testing

Goldens first, per the house rule:

- Core: index read/write, `byte(n)` fault out of range, literal
  assignability (and its compile error), append, slice, clone vs aliasing
  visibility, len, iteration, rendering, map[byte] keys, byte struct
  fields, zero values.
- Conversions: `[]byte(s)` and `str(b)` round-trip, invalid UTF-8 decode
  error, `int(b)`/`byte(n)`.
- Checker: byte/int mixing rejections, arithmetic rejection, `x byte`
  declarations everywhere types go.
- `py/` cases (NEVLA_TEST_PY=1): inbound `[]byte` to `len()` and
  `hashlib`, outbound from `bytes`/`bytearray`/`memoryview`, non-buffer
  object erroring, `byte` inbound as int.
- file: readbytes/writebytes round-trip including non-UTF-8 content.

## Spec sections touched (same commit as semantics, per house rule)

3.2 note, 5.1 (byte joins the scalar table), 5.9 (type syntax), 5.10
(literal assignability), 5.11 (zero value), 7.5, 7.6, 7.7 (conversion
table rows and the fallibility note), 7.9.2 (comparisons), 13.5 (both
tables), 14 (`len`, `append`, `clone`), 15.3 (file). Plus one new ADR:
byte/[]byte placement, the str-fallibility and no-arithmetic Go
deviations, zero-copy deferral. ADR 0019 consequences line is satisfied,
not amended.
