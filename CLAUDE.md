# rikki

Interpreted language: Go's discipline, CPython's ecosystem. Two binaries split
like uv/python: `rikki` (setup: new, py add, check, project run) and `tk`
(runner: `tk file.rk`, bare `tk` is the repl).

## Documents, and what governs what

- `language-spec.md` (repo root): the formal, normative spec. **Any change to
  language semantics MUST update this file in the same commit.** No exceptions;
  a semantics PR without a spec diff is incomplete.
- `tests/golden/`: the executable spec. Every language-visible behavior lands
  as a golden test first (`.rk` + `.out` for stdout, or `.err` for expected
  error substrings, one per line). A dir containing `main.rk` is one
  multi-file case. `py/` cases only run with `RIKKI_TEST_PY=1`.
- `docs/specs/2026-07-01-mongoose-v1-design.md`: design rationale, not
  normative (predates the rename to rikki; see ADR 0007). `docs/adr/`:
  decision records, append-only.

## Rules

- The full gate is `RIKKI_TEST_PY=1 cargo test`. Green before every commit.
- User programs may fault (reported error, rikki stack, nonzero exit) but
  must NEVER panic or abort the process. Every `unwrap`/`expect` reachable
  from user source is a bug.
- `src/bridge.rs` is the only file that may name pyo3. The GIL, conversions,
  and exception translation live there and nowhere else.
- Value semantics: `Value::clone` is a deep copy. `py` and ctx values are the
  documented reference exceptions. Do not add more without an ADR.
- Commit style: terse, verb-first, area prefix (`lexer:`, `parser:`, `check:`,
  `eval:`, `stdlib:`, `bridge:`, `cli:`, `spec:`).
