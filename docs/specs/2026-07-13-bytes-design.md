# byte and []byte: binary data as a compact list, shared across the bridge

Date: 2026-07-13. Status: approved. Source: backlog v1.1 top item; ADR 0019
promised bytes its own record. The shard workflow (read a shard, train from
it; create a shard) is the sizing case: the design must handle big data,
which forced the bridge posture below.

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

## Runtime representation: stable addresses by construction

`[]byte` gets a compact sibling of `Value::List` backed by a
reference-counted `Vec<u8>`; a bare `byte` value is `Value::Byte(u8)`. A
10MB body is 10MB contiguous, not ten million boxed `Value`s.

The property the bridge design rests on: **no operation in the language
reallocates a live buffer.** `b[i] = x` writes in place and never moves
memory; `append` is pure (ADR 0010), producing a new buffer and rebinding;
slices copy. A buffer's address is therefore stable from allocation to
death, except for one runtime optimization, governed by a lent flag:

- A buffer that is solely owned and has **never been lent** across the
  bridge may grow in place on `append` (amortized O(1)); building a shard
  by appending chunks is linear, not quadratic.
- A buffer that has **ever been lent** never grows in place: `append`
  copies. Lent views keep seeing the old buffer, which is exactly the
  rebinding semantics `append` already has. Stable addresses for every
  lent buffer, fast growth for every private one, no user-visible rule
  beyond "append rebinds."

## The bridge model for real data

This design sets the general model for data crossing the bridge, one rule
per kind (spec 13.5 gains this framing):

1. **Value types** (scalars, str, structs): convert, i.e. copy.
   Semantically lossless; values have no identity to preserve.
2. **Contiguous primitive buffers**: cross by reference, zero-copy, via
   CPython's buffer protocol. `[]byte` is the first member; compact
   numeric list types (`[]float64`, `[]int64` for data prep) inherit this
   exact design if demand arrives. This is the data plane.
3. **Structured containers** (lists, maps): copy today, per-element, as
   they always have (`to_py_depth` builds a fresh PyList/PyDict). CPython
   cannot view foreign memory as a `list`; the only by-reference option is
   a lazy proxy object (per-access dispatch and conversion), which trades
   one upfront copy for per-element round-trips — wrong for bulk data and
   breaks `isinstance(x, list)` consumers (json.dumps rejects
   non-lists). Recorded as the known upgrade if mutation-through-the-
   bridge ever earns its cost; not now.
4. **py handles**: already references, unchanged.

### []byte crossing: always a view

Inbound, a `[]byte` argument converts to a buffer-protocol object — a
small bridge-defined Python type holding the buffer's reference count and
exposing pointer+length. `memoryview`, `np.frombuffer`,
`torch.frombuffer`, `hashlib.update`, `f.write` consume it with zero
copies at any size. No size threshold, no opt-in: `[]byte` is a reference
type and it crosses like one — the first nevla type with reference
semantics through the bridge, which is its job. Mutations are visible
both ways (`b[i] = x` changes what a `frombuffer` tensor sees); nevla is
single-threaded and the GIL serializes Python, so there is no torn access
today. The concurrency proposal inherits the constraint.

The cost is ergonomic, accepted deliberately: a view is not a Python
`bytes`; libraries that `isinstance(x, bytes)`, key dicts on bytes, or
call `.decode()` need `bytes(b)` py-side — the one explicit copy, paid at
the call site that demands it.

Outbound, `[]byte(x)` on a `py` operand extracts from `bytes`,
`bytearray`, and `memoryview` (buffer protocol) with one copy in,
fallible like every outbound conversion. Zero-copy outbound (nevla
`[]byte` over Python-owned memory) is deferred: it gives `[]byte` a
second storage backing and complicates the CPython-free wasm build.
`byte(x)` on a `py` operand: deferred; `int(x)` then `byte(n)` covers it.

### The shard flows

Usage — the data plane never copies after the read:

```nevla
b := check file.readbytes("shard-00.bin")              // disk to ram, once
x := check torch.frombuffer(b, dtype: torch.float32)   // zero-copy view
```

(`torch.load`/`safetensors` paths, where the data never leaves Python at
all, remain the idiom for framework-format shards; `[]byte` serves the
raw-buffer and preprocessing cases.)

Prep — build privately (in-place growth), then write or lend:

```nevla
f := check file.create("shard-00.bin")
for sample := range n {
    chunk := encode_sample(sample)       // private buffer, fast appends
    check f.write(chunk)                 // lent per call, zero-copy
}
check f.close()
```

## Streaming

Streaming across the bridge is push-shaped: a loop over bounded chunks,
nevla driving, Python consuming synchronously inside each call
(`update`/`feed`/`write`-style APIs — hashing, compression, encryption,
incremental parsers). Buffer reuse works: writes to a lent buffer are
legal, visible, and never realloc, so reading into the same fixed buffer
each iteration is sound. Footgun to document: a `frombuffer` tensor
aliases the buffer, so the next chunk overwrites what the tensor sees;
`.clone()` py-side detaches.

Pull streaming — Python calling back into nevla (`json.load(f)` on a
nevla file-like, a DataLoader over a nevla dataset) — is not possible:
functions do not cross the bridge. That is a language-level gap
(callbacks across the bridge), out of scope here and recorded as a
follow-on; the idiom is inversion (nevla drives the loop) or keeping the
pipeline py-side behind a handle.

## stdlib: file (spec 15.3)

Whole-file pair:

- `file.readbytes(path str) ([]byte, error?)` — whole-file read; empty
  `[]byte` on error.
- `file.writebytes(path str, b []byte) error?` — create or truncate.

Chunked handles (v1, because without a chunk source push streaming is
theoretical). Importing `"file"` declares an opaque handle type `File`
(the `Proc` pattern of 15.12):

- `file.open(path str) (File, error?)` — read-only handle.
- `file.create(path str) (File, error?)` — write handle, create or
  truncate.
- `f.read(n int) ([]byte, error?)` — up to `n` bytes; empty `[]byte` with
  `none` error means end of file.
- `f.write(b []byte) error?` — write the whole buffer.
- `f.close() error?` — idempotent.

No append-mode handle, no seek, no read-into until something wants them.

## Out of scope, with re-entry paths

- Binary http. `Request`/`Response` declare `body str`; the flip to
  `[]byte` (break-early makes it available) or parallel fields is its own
  small design. Backlog keeps the line.
- fn across the bridge (callbacks, nevla file-likes, pull streaming):
  language feature with reentrancy questions; backlog entry.
- Lazy list/map proxies (containers by reference through the bridge):
  see the bridge model; adopt only when a use case demands mutation
  visibility and accepts per-access cost.
- Zero-copy outbound (`[]byte` over Python-owned buffers).
- `file.mapbytes` (mmap-backed read-only buffers): the page-cache upgrade
  for shard reading; natural follow-on, wants the read-only-buffer story.
- Hex integer literals (`0x89`). Lexer-only, orthogonal; backlog item.
- `[]int(b)` / `[]byte(xs)` cross-conversions: a `for` loop covers it.
- byte arithmetic and bit operators: compatible later.

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
- file: readbytes/writebytes round-trip with non-UTF-8 content; chunked
  read to EOF; create/write/close round-trip; write-after-close error.
- `py/` cases (NEVLA_TEST_PY=1, stdlib-only consumers): `len()` and
  `hashlib` over a lent view; `bytes(x)` py-side materialization;
  mutation visibility (nevla writes `b[i]`, a held `memoryview` sees it);
  append-after-lend detach (view sees old buffer); outbound from
  `bytes`/`bytearray`/`memoryview`; non-buffer object erroring; `byte`
  inbound as int; chunked hash loop equals whole-file hash.

## Spec sections touched (same commit as semantics, per house rule)

3.2 note, 5.1 (byte joins the scalar table), 5.9 (type syntax), 5.10
(literal assignability), 5.11 (zero value), 7.5, 7.6, 7.7 (conversion
table rows and the fallibility note), 7.9.2 (comparisons), 13.5 (the
per-kind bridge model, the view row, outbound row), 14 (`len`, `append`,
`clone`), 15.3 (file, including the File handle). Plus one new ADR:
byte/[]byte placement, always-view bridge crossing and the stable-address
argument, the lent flag, the str-fallibility and no-arithmetic Go
deviations, the buffer-family future, the deferred proxies. ADR 0019's
consequences line is satisfied, not amended.

docs/proposals/concurrency.md gains two lines in the same commit as the
bridge work: lent buffers are memory shared with Python and join lists in
whatever sharing story lands; the lent flag is the seam a future
synchronization story hooks into.
