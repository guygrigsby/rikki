# 15. Script stdlib: flag, os, time, regex, and file growth

Status: accepted 2026-07-10

## Context

Porting three everyday shell scripts to nevla (examples/scripts: got,
dev-watch, httpcheck) proved the language cannot write a basic script
without the py bridge: subprocess, glob, os, time, and even builtins
(a file handle for Popen) all had to come from Python. That fails the
pitch. A scripting language whose scripting story is "know the Python
stdlib" has no scripting story. The bridge also leaked friction into
ordinary code: no zero value for py blocks early error returns from
(py, error?) functions, and a bare fallible py statement like
time.sleep needs a wrapper function to satisfy mandatory handling.

Process spawning is the largest gap and gets its own ADR (0016). This
one covers the rest.

## Decision

Three new native modules and two file functions, shaped by the tenets:
options instead of fallbacks, data instead of mini-languages, Ctx for
anything that waits.

- `flag`: `struct Flag { name str, short str, fallback str, usage str }`,
  `flag.parse(flags []Flag) (Parsed, error?)`,
  `struct Parsed { values map[str]str, rest []str }`. Every flag has a
  long and short form; `--help`/`-h` are synthesized from the usage
  strings and come back as an error value carrying the usage text, as
  does an unknown flag. All values are strings; `int(x)`/`float(x)`
  conversions are already mandatory-checked, so no typed-flag machinery.
- `os`: `os.workdir() (str, error?)`, `os.env(name str) str?`. The
  option return replaces Python's get-with-default; narrowing does the
  work. Deliberately absent until something needs them: exit (an error
  from main is the exit status), setenv, hostname. (workdir was cwd
  until ADR 0017; same fossil as mtime.)
- `time`: `time.now() float` (epoch seconds), `time.clock() float`
  (monotonic seconds), `time.sleep(c Ctx, secs float) error?` (wakes
  early with the ctx error when the ctx ends, so poll loops die
  promptly on Ctrl-C), `time.parts(epoch float) Parts` with
  `struct Parts { year int, month int, day int, hour int, minute int,
  second int }`. No format mini-language; sprintf over Parts covers
  log timestamps. A layout string can be reconsidered when real code
  outgrows that.
- `file.glob(pattern str) ([]str, error?)` (`**` aware, results sorted)
  and `file.modified(path str) (float, error?)` (epoch seconds; was
  mtime until ADR 0017 made descriptive names a tenet). Folded into
  file rather than a new module; a glob is a question about the
  filesystem.
- `regex`: `regex.compile(pattern str) (Re, error?)` returning an
  opaque handle (reference semantics, like Ctx), with
  `re.matches(s str) bool`, `re.find(s str) Match?`,
  `re.find_all(s str) []Match`, `re.replace(s str, repl str) str`, and
  `struct Match { text str, start int, end int, groups []str }`.
  A match is data, absence is an option. Semantics are the Rust regex
  crate's: linear time, no backreferences or lookaround, and the
  compile error says so when a pattern wants them. Go made the same
  trade with RE2.

## Consequences

- The three example scripts rewrite to zero `import py`; the py bridge
  goes back to being the ecosystem door, not the scripting floor.
- Spec sections and golden tests land with the implementation, same
  commit, per the house rules.
- `time.sleep` taking a Ctx sets the pattern: any new stdlib call that
  blocks takes a Ctx. No timeouts as ad hoc float parameters.
- The flag error-not-print help behavior is 0012 applied to CLI UX:
  main decides what to do with usage text; the module never writes to
  stdout or exits.
