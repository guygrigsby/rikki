# 5. Errors may reach main

Status: accepted

## Context

`fn main() (error?)` plus `check` chains means an unhandled error propagates
to the top and the program dies with a message, which looks superficially
like Python's uncaught-exception blowup. Considered banning the error-
returning main signature and forcing explicit handling, with a `fatal(err)`
builtin as the only path to nonzero exit.

## Decision

Keep `fn main() (error?)`. Propagating to main is handling it: every hop was
marked with `check`, the error arrives typed, the runtime prints `error:
<msg>` and exits 1. Rust reached the same position (`fn main() -> Result`)
after watching everyone hand-write the unwrap-print-exit trailer.

## Consequences

- Scripts stay zero-ceremony; the shell contract (message on stderr, exit 1)
  is uniform and free.
- The Python comparison fails on the axis that matters: nothing in mongoose
  can fail unannounced. The blowup Python is hated for is the invisible kind.
- A `fatal(err)` / `exit(code)` builtin for explicit mid-program death stays
  available as a future addition; nothing here precludes it.
