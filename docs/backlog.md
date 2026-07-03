# Backlog

Not commitments, just recorded intent. Ordered roughly by expected pain.

## v1.1

- `bytes` type. Unblocks binary file and http bodies. Touches literals,
  indexing, conversions.
- `rikki fmt`. One true style, needs a lossless formatter.
- Repl typechecking (currently unchecked).
- Rethink strict copies everywhere (own design round + ADR superseding
  0004/0009; scope widened 2026-07-02 from closure capture to the whole
  value-semantics posture). Evidence so far: the by-value capture snapshot
  silently ate training-project writes twice (now a compile error, which
  is a tourniquet, not a fix); the sanctioned accumulator workaround (a
  captured py object) is clunky in practice because a bare py call
  statement in a callback still needs its chain consumed, forcing a
  fallible lambda + check + per-call handling just to acc.append(x); and
  http.stream returns an accumulated body as an apology for closures that
  cannot accumulate. Key framing for the round: Go, the taste reference,
  is not strict-copy either. Go structs copy on assign/pass, but slices,
  maps, and closures are reference-shaped. Candidate end state is the Go
  model wholesale: reference lists/maps, reference capture of free
  variables, per-iteration loop bindings (Go 1.22 lesson), structs and
  scalars staying value. Touches ADR 0004, 0009, spec 5/7.3/11, and the
  mutation-visibility goldens flip.
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
