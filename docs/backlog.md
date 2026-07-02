# Backlog

Not commitments, just recorded intent. Ordered roughly by expected pain.

## v1.1

- `bytes` type. Unblocks binary file and http bodies. Touches literals,
  indexing, conversions.
- `mongoose fmt`. One true style, needs a lossless formatter.
- Repl typechecking (currently unchecked).
- Diagnostics lack filenames in multi-file programs; line:col alone forces a
  grep across modules. Thread the origin file through the loader into Diag.
- Nested py chains as call arguments require an inner `check`
  (`f(check g())`); pervasive in torch code. Consider letting an enclosing py
  call absorb argument-chain fallibility.
- Infallible conversions typed as fallible: `float(int)` can never fail but
  returns `(float, error?)`, forcing `_, _ :=` boilerplate in numeric code.
  Type conversions by source: infallible pairs return one value.
- Dotted struct literals: `util.Pair{a: 1}` does not parse; module structs
  need factory functions today. Parser feature plus spec revert if wanted.

## v2

- Matrix operations: operator sugar over `py` (decided, no native matrix
  type). Add `@` matmul to the grammar (lexer token, binary op at mul
  precedence, py-only in the checker) and dispatch it through the bridge's
  binop path like `+ - * /` already do. Result: `y = w @ x + b` on live
  tensors; objects and speed stay PyTorch's.
- Branded py types (decided in principle). `pytype tensor = "torch.Tensor"`
  declares a nominal wrapper over a py reference, verified by isinstance
  through the bridge. Fallible assertion py to brand (`tensor(y)` returns
  `(tensor, error?)`), implicit widening brand to py, compile error on brand
  mismatch in signatures. Brands erode through dynamic ops (`w @ x` is py
  again); re-assert at function boundaries. Zero copies, reference semantics
  like py. Composes with the `@` sugar above.
- Concurrency. Bridge module is the single place GIL work lands.
- Bytecode VM if pure-mongoose loops ever hurt (recorded in ADR 0001).
- Copy-on-write values if profiling demands (ADR 0004).
- `==` structural equality for containers (contains already compares
  structurally; the operator stays scalar-only in v1).
