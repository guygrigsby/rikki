# Backlog

Not commitments, just recorded intent. Ordered roughly by expected pain.

## v1.1

- `bytes` type. Unblocks binary file and http bodies. Touches literals,
  indexing, conversions.
- `rikki fmt`: DONE 2026-07-09 (design:
  docs/specs/2026-07-09-fmt-design.md). Corpus-proven lossless (AST and
  comments preserved, idempotent over every golden); `rikki fmt --check`
  for gates; Format button in the playground.
- `rikki test`: DONE 2026-07-09 (design: the book's testing chapter,
  spec 15.6/17.7). Tests are fallible functions; error origins landed
  with it (spec 5.7). Deferred with re-entry paths: soft failures,
  test.run subtests, test.tmpdir, --json output.
- Version pinning (decided 2026-07-09): rikki.toml gains a rikki version
  stamp written by rikki new, with a warning on mismatch so future breaks
  say "built against 0.1.5" instead of mystifying compile errors; and
  `rikki py add` records the resolved version instead of "*" so the
  manifest reads true (rikki.lock already pins exactly).
- Source snippets in diagnostics (decided 2026-07-09): show the offending
  source line with a caret under file:line:col. Humans and the playground
  see it immediately; the agent hook feedback gets richer for free.
- Repl typechecking (currently unchecked).
- Gate the brew formula's python pin (2026-07-09): the tap formula pins
  python@3.12 by hand while the release matrix builds 3.10-3.13, a
  hand-synced pair (nothing-remembered rule). Either assert the pin
  appears in the matrix before publish, or generate the pin into the
  formula during the tap bump.
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
- Exported-identifier visibility: DONE 2026-07-09 (Go capitalization,
  spec 16.3; design in docs/specs/2026-07-09-exported-visibility-design.md).
  Prerequisite for packages below.

## Docs

New features are doc-first (the testing chapter was rikki test's design
doc; that's the pattern). This is the backlog of chapters the existing
language has already earned, ordered by user pain. Every chapter's
examples are compile-gated (tests/book.rs) and complete programs get
playground links automatically.

- The py bridge: the most alien surface and the biggest gap. Chains and
  the consumption rules, conversions both directions (the 13.5 tables in
  prose), kwargs, `@`, iteration, py assignment, `with`, `py(x)`, dotted
  imports, the manifest rule. One chapter, example-heavy.
- Errors in depth: check's exact semantics, wrap and cause chains,
  origins, recovery patterns, faults vs errors and why faults are
  uncatchable, the don't-blanket-check guidance with worked examples.
- Projects and dependencies: rikki.toml, py add and --module, the lock
  and venv lifecycle (fingerprints, auto-relock, rebuild-on-change),
  what sys.executable means inside a run.
- Types and the copy model: value vs reference kinds, clone, zero
  values, equality and contains, options and narrowing in full
  (invalidation, terminal narrowing).
- Structs, modules, and visibility: declarations, literals, file
  imports and namespacing, the capitalization rule, _test pairing.
- Tooling: fmt and the one true style, check, the repl, the nvim
  plugin, the agent story (AGENTS.md, the check-on-edit hook).
- Builtins and stdlib guide: printf verbs, container and string
  methods, math, file, ctx cancellation patterns, http including
  stream.
- Control flow: the three for forms, if and narrowing, break/continue,
  with.
- Design: everything-is-data, break-early, Go as the taste reference; a
  reader's digest of the ADRs.
- Gotchas / FAQ: the primer's list expanded (append is pure, map reads
  are optional, int/float don't mix, py comparisons yield py, ...).
- Reference generation, later stages (2026-07-09): the book's Builtins
  and Standard library chapters are generated from spec ch 14/15 at
  build time. Stage two is sigs.rs becoming a declarative table feeding
  both the checker and the docs (everything-is-data applied to the
  stdlib surface). Stage three is `rikki doc`: doc comments in .rk
  source generating package docs, the true godoc analog, when packages
  exist.

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
- Concurrency: SPEC SOON (escalated 2026-07-09), implement later. A
  design round before more v1.1 features lock in semantics that
  concurrency would have to break: the reference types (lists, maps, py,
  ctx) need a sharing/synchronization story, the GIL bounds what py work
  can ever parallelize (bridge stays the single place GIL work lands),
  and ctx is already shaped like the cancellation primitive. The point is
  to not paint over the door we intend to walk through.
- Bytecode VM if pure-mongoose loops ever hurt (recorded in ADR 0001).
- Copy-on-write values if profiling demands (ADR 0004).
- `==` structural equality for containers (contains already compares
  structurally; the operator stays scalar-only in v1).
