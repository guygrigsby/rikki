# nevla

Interpreted language: Go's discipline, CPython's ecosystem. Two binaries split
like uv/python: `nevla` (setup: new, py add, check, project run) and `nv`
(runner: `nv file.nv`, bare `nv` is the repl).

## Documents, and what governs what

- `language-spec.md` (repo root): the formal, normative spec. **Any change to
  language semantics MUST update this file in the same commit.** No exceptions;
  a semantics PR without a spec diff is incomplete.
- `tests/golden/`: the executable spec. Every language-visible behavior lands
  as a golden test first (`.nv` + `.out` for stdout, or `.err` for expected
  error substrings, one per line). A dir containing `main.nv` is one
  multi-file case. `py/` cases only run with `NEVLA_TEST_PY=1`.
- `docs/specs/2026-07-01-mongoose-v1-design.md`: design rationale, not
  normative (predates the rename to nevla; see ADR 0007). `docs/adr/`:
  decision records, append-only.

## Rules

- The full gate is `NEVLA_TEST_PY=1 cargo test`. Green before every commit.
- User programs may fault (reported error, nevla stack, nonzero exit) but
  must NEVER panic or abort the process. Every `unwrap`/`expect` reachable
  from user source is a bug.
- `src/bridge.rs` is the only file that may name pyo3. The GIL, conversions,
  and exception translation live there and nowhere else.
- The copy model is Go's split (ADR 0010): scalars, str, structs, tuples,
  and errors are value types; lists, maps, fn, py, and ctx are reference
  types behind shared cells. `Value::clone` copies by kind (cheap; never a
  deep copy). Do not move a type across the split without an ADR.
- Commit style: terse, verb-first, area prefix (`lexer:`, `parser:`, `check:`,
  `eval:`, `stdlib:`, `bridge:`, `cli:`, `spec:`).
- Concurrency is deferred, not ignored: any concurrency consideration hit
  while working (a constraint, an API that must stay wrappable, a user
  waiting on it) gets appended to `docs/proposals/concurrency.md` in the
  same commit as the work that surfaced it.
