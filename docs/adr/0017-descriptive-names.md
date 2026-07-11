# 17. Names are descriptive

Status: accepted 2026-07-10

## Context

The script stdlib design (ADR 0015) briefly contained `file.mtime`, an
abbreviation inherited from `st_mtime`, a Unix fossil that means nothing
unless you already know stat(2). The rest of the file module is plain
words: read, write, exists, list. The fossil stood out, and the fix
(`file.modified`) was obviously better the moment it was spoken. That
instinct deserves to be a rule so it does not need re-arguing per name.

## Decision

Names are descriptive. A longer clear name beats a shorter cryptic one,
every time, in the stdlib, the spec, and the implementation. Very Go:
`ReadFile`, `ModTime`, and `LastIndex` over `fread`, `mtime`, and
`rindex`.

Boundaries:

- Domain-standard names that are more recognizable than their expansions
  stay: `http`, `gpu`, `regex`, `ctx`, `min`, `max`, `abs`, `pi`.
  The test is whether the short form is the term practitioners actually
  say, not whether an expansion exists.
- Language keywords are out of scope (`fn`, `str`, `int` are syntax, not
  API), as is Go-style brevity for local variables inside a function.
- Ad hoc abbreviations, smashed words, and stat-field fossils are out:
  no `mtime`, no `getenv`, no `strcmp`-style names, ever.

Applied immediately: `file.mtime` becomes `file.modified`, and
`os.cwd` (same fossil, different syscall) becomes `os.workdir`.

## Consequences

- ADR 0015's surface is renamed before implementation; the ADR text is
  updated in place since nothing was built against the old names.
- New API review includes a naming pass: any abbreviation must clear
  the term-of-art bar or spell itself out.
- When a module wraps a Unix or Python concept, nevla names the concept,
  not the historical identifier.
