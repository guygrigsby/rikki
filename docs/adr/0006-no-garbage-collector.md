# 6. No garbage collector

Status: accepted

## Context

Interpreted languages usually need a GC. Mongoose's memory story was never
explicitly decided; it turns out to be settled by ADR 0004 (value semantics).

## Decision

No garbage collector. Memory management is Rust ownership plus reference
counting, and this is complete, not approximate:

- Value semantics means no object graph. Values cannot contain references to
  other mongoose values, so there is nothing to trace; scope exit frees.
- The shared exceptions (closures via Rc, py and ctx handles via Arc) are
  immutable after construction and can only reference data created before
  them, so the reference graph is a DAG and cycles are unrepresentable.
  Refcounting therefore never leaks.
- Python objects belong to CPython's own GC; the bridge holds plain refcounts
  on them.

## Consequences

- No pauses, no tuning, no finalizers.
- Copy-on-write (backlog) preserves the property: shared COW state is
  immutable by definition.
- Any future feature introducing mutable shared references (true reference
  types, shared mutable captures) breaks the theorem and forces a cycle
  collector; that feature must supersede this ADR explicitly.
