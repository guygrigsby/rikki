# 3. Errors as values, options, no nil, no exceptions

Status: accepted

## Context

Two of the four target failure modes: error handling chaos (exceptions from anywhere, bare except) and runtime type surprises (None/nil landmines). Go's errors-as-values discipline works but its `nil` reintroduces the landmine and dropped errors compile fine.

## Decision

- Fallible functions return `(T, error?)`. Dropping an error is a compile error.
- `check expr` propagates the error from the enclosing function, keyword taken from the Go 2 draft. No `try`; no exception vocabulary.
- No exceptions, no user-visible panic. Runtime faults (division by zero, index out of bounds) stop the program with a reported stack, never a process crash.
- No nil. Absence is the option type `T?` with literal `none`, unwrapped by flow narrowing: after `if x != none`, `x` is `T` in that branch.

## Consequences

- Every fallible call is visibly handled at the call site: `check`, destructure + narrow, or a compile error.
- One concept (options) covers absent values and the error slot.
- Flow narrowing puts real dataflow analysis in the typechecker; scope kept to `== none` / `!= none` conditions and diverging branches.
