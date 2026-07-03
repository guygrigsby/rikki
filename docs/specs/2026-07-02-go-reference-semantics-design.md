# The Go copy model: reference containers and reference capture

Date: 2026-07-02. Status: implemented (ADR 0010). Supersedes the value-semantics
posture of ADR 0004 and the capture model of ADR 0009 (a new ADR lands with
the implementation; this doc is its working draft).

## Why

Strict copies everywhere was stricter than Go, the taste reference, and the
strictness concentrated pain exactly where Go chose references: containers
and closures. Evidence: capture snapshots silently ate training-project
writes twice; the sanctioned workaround (a captured py object) drags a
fallible lambda and per-call `check` into every callback; http.stream
returns an accumulated body as an apology; and the interpreter grew borrow
fast-paths just to dodge O(n) copies on every read. Go's actual discipline
is a split, and everyone who writes Go knows it cold: scalars and structs
copy, slices, maps, and closures alias.

## The model

Two kinds of types, Go's split exactly:

**Value types** (copy on assignment, argument passing, return, capture):
`int`, `float`, `bool`, `str` (immutable, like Go strings), `error`,
structs, tuples, `none`. Struct copies are shallow in Go's sense: fields
that are reference types copy the reference, so the copies alias those
containers. This is precisely how a Go struct holding a slice behaves.

**Reference types** (assignment etc. copy the reference; one underlying
object): `[]T`, `map[K]V`, `fn`, `py`, Ctx. The last three already were;
lists and maps join them.

```
xs := [1, 2, 3]
ys := xs
ys[0] = 9
print(xs[0])   // 9: ys and xs are the same list
```

**Zero values improve on Go:** the zero of `[]T` and `map[K]V` is a fresh
empty container, usable immediately. rikki has no nil and gains none;
there is no nil-map write panic to import.

**Explicit copying is stdlib, as in Go** (`slices.Clone`, `maps.Clone`):
a `clone(x)` builtin returns a one-level shallow copy of a list or map
(elements that are themselves references still alias). Deep copies, as in
Go, are written when needed. `clone` of a value type is a compile error
(it already copies; the call would lie).

## Closures

Function literals capture their free variables by reference, Go's
semantics: the closure and the enclosing scope share the variable; writes
in either direction are visible in both. Capture is per-variable (free-var
analysis of the body), not today's whole-scope snapshot, so closures keep
alive exactly what they use. Loop iteration variables are per-iteration
(Go 1.22 semantics from day one; the interpreter already binds fresh
per-round scopes, which becomes observable now).

Consequences worth naming:

- The captured-write compile error added 2026-07-02 is deleted; writes now
  escape by design. Its goldens flip from `.err` to `.out`.
- http.stream's handler can accumulate; the accumulated-body return
  becomes redundant (kept for compatibility until a deliberate removal).
- A captured scalar is shared: the spec 7.3 example flips its output.

## Equality and cycles

`==` stays scalar-only. Go does not compare slices or maps with `==` and
neither do we; `contains` keeps structural comparison. Aliasing makes
cyclic data constructible for the first time (a struct field list holding
structs whose own list field aliases an ancestor), so every deep recursion
over values — render, `contains`/structural equality, `clone` is exempt
(one level) — gets a depth cap that faults ("value too deep or cyclic"),
in the same spirit as the call-recursion limit. Silent infinite loops and
stack overflow both stay impossible.

## Iteration under mutation

Defined, not undefined, and Go-shaped:

- Ranging a list fixes the length at loop entry and reads elements by
  index per round: element writes during iteration are visible, appends
  during iteration are not visited. (Aliased appends grow the shared list;
  the loop still stops at the entry length.)
- Ranging a map snapshots the key list at entry; entries deleted
  mid-iteration are skipped, entries added are not visited.
- The iteration variables remain fresh copies per round (value types) or
  aliases (reference types), consistent with everything above.

## Implementation shape

- `Value::List(Rc<RefCell<Vec<Value>>>)`, `Value::Map(Rc<RefCell<IndexMap
  <MapKey, Value>>>)`. `Value::clone` becomes the natural derive: cheap,
  reference-correct. The deep-copy discipline, the read fast-paths, and
  most of the interpreter's copy choreography dissolve.
- Scope slots become shared cells (`Rc<RefCell<Value>>`) so capture can
  take the variable, not its value. ClosureData holds the cells for its
  free variables.
- No borrow may be held across an eval/exec call; borrow conflicts must be
  unreachable by construction (audited at range loops, assign descent,
  binop). The no-panic rule stands: any residual `try_borrow` failure is
  an interpreter-bug fault, never an abort.
- The bridge is untouched: py conversion already walks values; it borrows
  containers momentarily like any other reader. `src/bridge.rs` remains
  the only pyo3 file.

## Blast radius

- Spec: chapter 5 (types), 7.3 (function literals), 7.9/11 (operators,
  equality), 8.7 (range), the value-semantics chapter, http.stream's note.
  Every "deep copy" sentence is revisited in one pass.
- ADR 0010 records the model; 0004 and 0009 are superseded (py and ctx
  stop being exceptions and become instances of the reference kind).
- CLAUDE.md's "Value::clone is a deep copy" rule is rewritten to the split.
- Goldens: mutation-visibility cases flip deliberately, one commit each
  with the spec diff; the captured-write `.err` cases become positive
  tests. lmtk must stay green (it mostly gains).
- The checker is nearly untouched: types do not change, runtime meaning
  does. `clone` gets a signature; the captured-write diagnostic is
  removed.

## Rejected

- Reference capture only (containers stay value): fixes accumulators but
  keeps the container copies and the divergence from Go; two mental
  models.
- COW as semantics: performance without the aliasing meaning; accumulators
  stay broken.
- Everything-reference (Python model): loses the scalar/struct copy
  guarantees that make rikki programs auditable; off-brand.
