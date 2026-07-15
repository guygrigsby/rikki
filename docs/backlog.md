# Backlog

Not commitments, just recorded intent. Ordered roughly by expected pain.

## v1.1

- byte and `[]byte`: DONE 2026-07-14 (ADR 0021, ADR 0022; design:
  docs/specs/2026-07-13-bytes-design.md). byte scalar (compare-only, no
  arithmetic) plus `[]byte` as a compact list type; always-view zero-copy
  bridge crossing via the buffer protocol (stable addresses by
  construction: `append` always copies rather than mutating in place, so
  a lent buffer's memory never moves — ADR 0022 supersedes ADR 0021's
  lent-flag in-place-growth optimization, which proved unsound: a
  refcount check cannot tell "the caller is rebinding this name"
  (`b = append(b, x)`, safe) from "the caller is keeping it and binding
  elsewhere" (`c := append(b, x)`, not safe), so growth is unconditional
  copy now, matching every other `[]T`); chunked file handles
  (open/create/read/write/close, the `Proc` opaque-handle pattern). Sets
  the bridge model for real data: values copy, primitive buffers cross by
  reference, containers copy (lazy proxies are the recorded upgrade), py
  handles unchanged. Follow-ons tracked as their own entries below:
  compact numeric buffers, fn across the bridge, binary http,
  `file.mapbytes`, hex integer literals.
- Compact numeric buffers `[]float64`/`[]int64` (2026-07-13): the byte
  design is written to be the template (data prep building tensors
  nevla-side, `np.frombuffer` zero-copy). Build when a data-prep flow
  wants it; no new design round needed, inherit the bytes ADR.
- fn across the bridge (2026-07-13): Python cannot call back into nevla,
  so pull-shaped py APIs (file-likes, DataLoader datasets, json.load on
  a stream) are inexpressible; the idiom is inversion (nevla drives,
  push APIs receive). A callable-proxy design owns the reentrancy
  questions when the inversion idiom stops being enough.
- Binary http (2026-07-13): `Request`/`Response` declare `body str`;
  binary bodies need the break-early flip to `[]byte` or parallel
  fields. Scoped out of the bytes design; needed before nevla can
  download a shard itself.
- `file.mapbytes` (2026-07-13): mmap-backed read-only buffers, the
  page-cache upgrade for shard reading; wants a read-only-buffer story
  first.
- Hex integer literals (2026-07-13): `0x89` for byte-heavy code; decimal
  grates in `[]byte{137, 80, 78, 71}`. Lexer-only, orthogonal.
- proc module: DONE 2026-07-10 (ADR 0016, spec 15.12). run/exec/start/
  attach with runtime-pumped pipes; got, dev-watch, and httpcheck are
  zero import py.
- printf zero-padding: `%02d` parses as width 2 and space-pads; Go
  zero-pads. Surfaced writing timestamps (dev_watch two() helper is
  the workaround). Flags (`0`, `-`, `+`) are one contained lexer
  change in the verb parser.
- Re-evaluate the two-binary split (2026-07-10): maybe just `nv`. The
  uv/python analogy motivated `nevla` (setup) + `nv` (runner), but the
  owner cannot remember the long name and calls the language nv, the
  verb set has grown (run/check/fmt/imports/test/new/py/repl), and one
  binary with subcommands plus `nv file.nv` as the bare fast path may
  serve better. Touches packaging (wheel entry points, brew), docs,
  the shebang story, and the hook template; wants its own ADR either
  way.
- Import aliasing (2026-07-11): `import py "os"` and std `os` cannot
  coexist (duplicate-name diag, deliberate), and python's stdlib names
  collide with nevla's own (os, time). repair-wheel.nv had to move to
  an env-var interface because argv lives on std os. Go's answer is
  `import alias "path"`; nevla wants the same and the spec slot is
  ready (ImportSpec).
- `defer` or an answer to it: `with` (py-only) got a keyword while
  native code has no cleanup construct; gpu leans on process-exit
  release. The audit flagged the asymmetry (2026-07-10).
- `nevla fmt`: DONE 2026-07-09 (design:
  docs/specs/2026-07-09-fmt-design.md). Corpus-proven lossless (AST and
  comments preserved, idempotent over every golden); `nevla fmt --check`
  for gates; Format button in the playground.
- `nevla test`: DONE 2026-07-09 (design: the book's testing chapter,
  spec 15.6/17.7). Tests are fallible functions; error origins landed
  with it (spec 5.7). Deferred with re-entry paths: soft failures,
  test.run subtests, test.tmpdir, --json output.
- Version pinning, nevla half: DONE 2026-07-09 (spec 17.4 area; nevla
  new stamps `nevla = "x.y.z"`, mismatch warns, never blocks). Still
  open: `nevla py add` recording the resolved version instead of "*" —
  needs an upgrade-path decision first (does re-adding refresh the pin?).
- Source snippets in diagnostics (decided 2026-07-09): show the offending
  source line with a caret under file:line:col. Humans and the playground
  see it immediately; the agent hook feedback gets richer for free.
- Repl typechecking (currently unchecked): byte/int comparisons compare
  numerically rather than by declared type in the repl (2026-07-14, task 2
  review), same as the checked path's literal implicit but with no range or
  type check at all. Close this note when repl checking lands.
- Graceful bridge startup (2026-07-09): a missing CPython stdlib at py
  init is a fatal abort today (with an actionable warning printed above
  it since 0.1.13). Full fix is PyConfig-based init through the ffi,
  which returns a status instead of aborting, so the failure can become
  an ordinary runtime error.
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

New features are doc-first (the testing chapter was nevla test's design
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
- Projects and dependencies: nevla.toml, py add and --module, the lock
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
  stdlib surface). Stage three is `nevla doc`: doc comments in .nv
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
- Packages: export and depend on nevla code across projects (2026-07-09).
  The Go shape throughout, it is already muscle memory: capitalization
  visibility (v1.1 item above, the breaking prerequisite), git-based deps
  over any registry (`[deps] torchkit = { git = "...", rev = "..." }` in
  nevla.toml, vendored under .nevla/, imported by name), a package is a
  directory of .nv files without a main. The genuinely novel piece is
  transitive py-deps: a package's `[py-deps]` merge into the consumer's
  requirement set and one uv resolve covers the union. Natural first
  artifact: typed facades over python packages (the lmtk tracker/loader
  wrappers). Trigger: the first time a second project wants lmtk's nevla
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
