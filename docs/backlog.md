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
- Context managers: DONE 2026-07-09 (`with expr { }`, py-only, spec 8.9;
  design in docs/specs/2026-07-09-context-managers-design.md). Deferred
  from it: binding form (`with x := expr`), __exit__ on faults.

- Optional py imports (lmtk, 2026-07-09): eager-fatal `import py` makes
  optional deps all-or-nothing. importlib.import_module through the bridge
  already expresses runtime-optional imports; revisit first-class syntax
  only if that pattern spreads.
- Exported-identifier visibility, the Go way (2026-07-09): capitalized =
  exported, lowercase = module-private, no new keywords. Breaking (file
  modules currently expose everything, e.g. util.twice in the goldens),
  which is exactly why it goes NOW and not with packages later: the
  standing posture is to ship breaking fundamental changes early, before
  anyone depends on the old shape. Prerequisite for packages below.

## v2

- Matrix operations: DONE 2026-07-02 (`@`, py-only, spec 7.9/13.2).
- Branded py types (decided in principle). `pytype tensor = "torch.Tensor"`
  declares a nominal wrapper over a py reference, verified by isinstance
  through the bridge. Fallible assertion py to brand (`tensor(y)` returns
  `(tensor, error?)`), implicit widening brand to py, compile error on brand
  mismatch in signatures. Brands erode through dynamic ops (`w @ x` is py
  again); re-assert at function boundaries. Zero copies, reference semantics
  like py. Composes with the `@` sugar above.
- Packages: export and depend on rikki code across projects (2026-07-09).
  The Go shape throughout, it is already muscle memory: capitalization
  visibility (v1.1 item above, the breaking prerequisite), git-based deps
  over any registry (`[deps] torchkit = { git = "...", rev = "..." }` in
  rikki.toml, vendored under .rikki/, imported by name), a package is a
  directory of .rk files without a main. The genuinely novel piece is
  transitive py-deps: a package's `[py-deps]` merge into the consumer's
  requirement set and one uv resolve covers the union. Natural first
  artifact: typed facades over python packages (the lmtk tracker/loader
  wrappers). Trigger: the first time a second project wants lmtk's rikki
  code. Explicitly NOT a stability commitment: packages do not slow
  language change pre-adoption.
- Concurrency. Bridge module is the single place GIL work lands.
- Bytecode VM if pure-mongoose loops ever hurt (recorded in ADR 0001).
- Copy-on-write values if profiling demands (ADR 0004).
- `==` structural equality for containers (contains already compares
  structurally; the operator stays scalar-only in v1).
