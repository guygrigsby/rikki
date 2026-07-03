# 10. The Go copy model: reference containers and reference capture

Status: accepted (supersedes the value-semantics posture of ADR 0004 and
the capture model of ADR 0009)

## Context

v1 copied everything deeply: assignment, argument passing, capture. That
was stricter than Go, the taste reference, and the strictness concentrated
pain exactly where Go chose references. The training project hit it
repeatedly: capture snapshots silently ate accumulator writes twice, the
sanctioned workaround (a captured py object) dragged a fallible lambda and
per-call check into every callback, http.stream returned an accumulated
body as an apology for closures that cannot accumulate, and the
interpreter grew borrow fast-paths to dodge O(n) copies on every read.
Design doc: docs/specs/2026-07-02-go-reference-semantics-design.md.

## Decision

Split the types Go's way. Scalars, strings, structs, tuples, and errors
are value types: they copy on assignment, argument passing, return,
iteration binding, and capture, with struct copies shallow in Go's sense.
Lists, maps, fn, py, and Ctx are reference types: one underlying object,
copies of the reference. py and Ctx stop being documented exceptions and
become ordinary members of the reference kind.

Consequences chosen deliberately:

- `clone(x)` (one-level, Go's slices.Clone/maps.Clone) is the explicit
  copy; `append` stays pure so growth propagates only by rebinding;
  `m.delete(k)` mutates in place like Go's delete.
- `==` stays scalar-only, as in Go.
- Iteration under mutation is defined: list length fixes at loop entry,
  map keys snapshot at entry.
- Cyclic values become constructible; every deep walk is depth-capped
  (structural compare and bridge conversion fault "value too deep or
  cyclic", rendering truncates, self-aliasing assignment paths fault).
  RefCell conflicts are unreachable or converted to faults; the no-panic
  rule stands.
- Closures capture their free variables by reference (Go's semantics),
  with per-iteration loop bindings (Go 1.22 lesson). The captured-write
  compile error of 2026-07-02 is deleted; those writes are the point.

## Consequences

- Runtime representation: lists and maps live behind Rc<RefCell<..>>;
  Value::clone is the cheap reference-correct derive; scope slots become
  shared cells so capture can take the variable.
- Spec chapters 1, 5, 7.3, 8.7, 11, 14 rewritten; mutation-visibility
  goldens flipped with the semantics commit that changed them.
- Programs relying on implicit container copies must call clone; the
  golden suite and lmtk were migrated in the same change.
