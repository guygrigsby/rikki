# 9. Closures share their immutable captured environment

Status: accepted

## Context

ADR 0004 fixes value semantics: `Value::clone` is a deep copy, with `py` and
ctx values as the documented reference exceptions, and CLAUDE.md forbids
growing that list without an ADR. A code audit found a third de facto
exception that predates this record: `FnRef::Closure(Rc<ClosureData>)`.
Cloning a function value shares the `Rc`; the captured environment is not
deep-copied.

## Decision

Keep the sharing and document it as deliberate. `ClosureData` (params,
declared return types, body, captured scope snapshot) is immutable after
capture: calls copy captured values into a fresh scope and discard writes, so
no program can observe whether two function values share their capture.
Sharing immutable data is observationally identical to copying it, at none of
the cost; a lambda passed into `map` over a large list would otherwise
deep-copy its whole environment once per element.

The invariant that makes this sound: nothing mutates through the `Rc`.
`ClosureData` has no interior mutability and the interpreter never obtains a
mutable reference to a shared capture. Any future feature that lets a closure
write to its captured environment (mutable upvalues) breaks the invariant and
must revisit this ADR.

## Consequences

- The reference-exception list in ADR 0004 is unchanged in spirit: closures
  are not observably reference-typed, so the language surface still has
  exactly two reference exceptions (`py`, ctx).
- `value.rs` documents the sharing at the `FnRef::Closure` variant.
- Mutable captures, if ever proposed, require either `Rc::make_mut`
  copy-on-write or a return to deep copies, decided in a superseding ADR.
