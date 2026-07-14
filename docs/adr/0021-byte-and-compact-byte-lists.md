# 21. byte and compact byte lists

Status: accepted 2026-07-13

## Context

Binary data was the top backlog item and ADR 0019 promised bytes a
record of their own. The sizing case is the shard workflow: read a
multi-megabyte buffer, hand it to torch or hashlib without copying,
build one by appending chunks. The design
(`docs/specs/2026-07-13-bytes-design.md`) rejected an immutable value
`bytes` type (loses in-place mutation, forces a second type when a
buffer needs editing) and an element type legal only inside `[]`
(breaks the compositional type grammar).

## Decision

Two additions, both ordinary members of existing kinds:

- `byte` is a predeclared scalar value type holding 0 through 255,
  zero value `0`, legal everywhere a type is legal, including map
  keys. It is compare-only in v1: `==`, `!=`, and the ordered
  comparisons work, arithmetic does not — math means `int(b)` first.
  Deviation from Go (uint8 has full arithmetic and wraps); recorded
  here per ADR 0013. This sidesteps wrap-vs-fault and byte/int mixing
  rules entirely; adding arithmetic later is compatible.
- `[]byte` is the list type over it, every chapter 11 list rule
  verbatim: reference type (ADR 0010 placement), aliasing, pure
  `append`, `clone` one-level, slices copy, not a map key, no `==`.
- Integer literals in range assign to `byte` positions bare
  (`[]byte{137, 80}`, `b[i] = 255`); an out-of-range literal is a
  compile error. This is the only implicit; an `int` variable never
  assigns to `byte` without conversion.
- `byte(n)` from `int` faults when `n` is outside 0..255. Go
  truncates silently; nevla does not do silent.
- `str(b)` from `[]byte` is fallible: nevla strings are characters
  (ADR 0019), so invalid UTF-8 cannot pass. Deviation from Go, where
  `string(b)` accepts any bytes. First non-`py` operand for which
  `str(x)` can fail.

Runtime and bridge:

- `[]byte` gets a compact representation backed by a reference-counted
  `Vec<u8>`; a 10MB body is contiguous, not ten million boxed values.
- No language operation reallocates a live buffer: index-assign writes
  in place, `append` rebinds, slices copy. Addresses are therefore
  stable, which is what the bridge posture rests on. One optimization
  is governed by a lent flag: a buffer never lent across the bridge
  may grow in place on `append`; a buffer ever lent never does, so
  lent views keep seeing the old buffer — exactly `append`'s rebinding
  semantics.
- Crossing the bridge, `[]byte` is always a view: a buffer-protocol
  object over the same memory, zero-copy at any size, no threshold, no
  opt-in. It is a reference type and crosses like one. Libraries that
  demand a real `bytes` pay the one explicit copy py-side
  (`bytes(b)`). Outbound, `[]byte(x)` extracts from `bytes`,
  `bytearray`, and `memoryview` with one copy.
- This sets the per-kind bridge model (spec 13.5): value types copy,
  contiguous primitive buffers cross by reference, structured
  containers copy, `py` handles are already references. Compact
  numeric lists (`[]float64`, `[]int64`) inherit the buffer design if
  demand arrives.

## Consequences

- Deferred, with re-entry paths in the design doc: lazy list/map
  proxies through the bridge, zero-copy outbound over Python-owned
  memory, byte arithmetic and bit operators, hex literals, binary
  http bodies, fn across the bridge (pull streaming).
- Lent buffers are memory shared with Python; the lent flag is the
  seam a future concurrency story hooks into
  (`docs/proposals/concurrency.md`).
- ADR 0019's promise of a bytes record is satisfied, not amended:
  `str` semantics are unchanged.
