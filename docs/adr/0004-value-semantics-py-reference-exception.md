# 4. Value semantics, with py as the reference exception

Status: accepted

## Context

Python's silent weirdness list is dominated by aliasing: mutable default args, shared list references, spooky mutation at a distance. Go's slices carry the same trap. Fourth target failure mode.

## Decision

Assignment, argument passing and return copy the value. Naive deep copy in v1 (`Value: Clone` where clone is deep); copy-on-write later only if profiling demands it. Closures capture by value at creation. The single exception: `py` values are references to live Python objects and clone as references.

## Consequences

- Aliasing bugs are unrepresentable in pure mongoose code.
- Deep copy of large containers is O(n) per assignment; acceptable for scripts, COW is the recorded optimization path.
- "Mutating" container methods are pure (return new values); index assignment mutates only the local copy.
- The `py` exception is visible in the type, so which rules a value plays by is always known statically.
