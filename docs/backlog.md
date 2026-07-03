# Backlog

Not commitments, just recorded intent. Ordered roughly by expected pain.

## v1.1

- `bytes` type. Unblocks binary file and http bodies. Touches literals,
  indexing, conversions.
- `rikki fmt`. One true style, needs a lossless formatter.
- Repl typechecking (currently unchecked).
- Go closure capture semantics (own design round + ADR superseding 0009).
  The by-value snapshot of spec 7.3 has silently eaten writes in the
  training project twice; the interim compile error on captured writes
  makes it loud, the real fix is reference capture of free variables with
  per-iteration loop bindings (Go 1.22 lesson baked in from day one). Also
  un-hacks the http.stream accumulated-body apology.
- Context managers, from the training project (2026-07-02):
  torch.no_grad() / autocast() currently need set_grad_enabled or explicit
  __enter__/__exit__ calls. Likely a py-only `with expr { }` statement;
  the open semantics question is what __exit__ sees during a check early
  return or a fault. Queued behind the capture redesign.

## v2

- Matrix operations: DONE 2026-07-02 (`@`, py-only, spec 7.9/13.2).
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
