# Backlog

Not commitments, just recorded intent. Ordered roughly by expected pain.

## v1.1

- `bytes` type. Unblocks binary file and http bodies. Touches literals,
  indexing, conversions.
- `rikki fmt`. One true style, needs a lossless formatter.
- Repl typechecking (currently unchecked).
- Rethink strict copies everywhere: DONE 2026-07-02 as the Go model
  wholesale (ADR 0010, spec chapter 11): reference lists/maps, reference
  capture, structs and scalars staying value.
- Context managers, from the training project (2026-07-02):
  torch.no_grad() / autocast() currently need set_grad_enabled or explicit
  __enter__/__exit__ calls. Design approved 2026-07-09
  (docs/specs/2026-07-09-context-managers-design.md): py-only
  `with expr { }`; __exit__ sees a synthesized exception on error-carrying
  returns, None otherwise; faults skip it.

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
