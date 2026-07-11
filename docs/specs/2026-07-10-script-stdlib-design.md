# Script stdlib design: flag, os, time, regex, file growth

Design for ADR 0015 (script stdlib) with the ADR 0016/0017 cascades.
This is the plan; the spec is normative once the sections land, and the
golden tests are the executable word. Routine per module: goldens, spec
section, implementation, one commit each.

## Decisions pinned here

- One time currency: `int` nanoseconds, everywhere (ADR 0015). Exact in
  i64 until 2262. Duration constants make it readable. `ctx.timeout`
  migrates to it (task ordering: time module first, so the migration's
  docs and examples can say `30 * time.second`).
- Names are descriptive (ADR 0017): `file.modified`, `os.workdir`.
- Local time for `time.parts`: script log stamps are for humans at the
  machine. Goldens assert invariants (`p.year >= 2026`), not exact
  values, since output depends on the host timezone; exact conversion
  is pinned by Rust unit tests with a fixed offset.
- wasm honesty (playground doctrine): anything meaningless in the
  browser reports its absence. `os.workdir`, `os.env`, `file.glob`,
  `file.modified`, `time.sleep` return errors or faults per their
  shape; `regex` and `flag` work everywhere; `time.now` works,
  `time.clock` works via the JS performance clock if cheap, else
  reports absence like `ctx.timeout` does today.

## Module surfaces

### time (spec 15.8)

```
time.now() int                         epoch nanoseconds
time.clock() int                       monotonic nanoseconds
time.sleep(c Ctx, d int) error?        none on full sleep; the ctx
                                       error on early wake; negative d
                                       sleeps 0
time.parts(epoch int) Parts            local time
struct Parts { year int, month int, day int,
               hour int, minute int, second int }
```

Constants (`Member::Const(Int)`): `nanosecond` 1, `microsecond` 1e3,
`millisecond` 1e6, `second` 1e9, `minute` 60e9, `hour` 3600e9.

Sleep implementation: sleep in short slices (~50ms) checking
`CtxInner::err()` between slices, so SIGINT and deadlines wake it
without threads. Slice granularity is an implementation detail the
spec does not promise.

### ctx migration (spec 15.4 edit)

`ctx.timeout(parent Ctx, d int) Ctx`. Negative clamps to 0 (unchanged
in spirit). The float overflow fault paragraph dies with the float; an
i64 nanosecond count always fits a Duration. Callers to update: spec
15.4/15.5 examples, chat and httpcheck examples, stdlib goldens.

### os (spec 15.9)

```
os.workdir() (str, error?)     absolute path; error if the dir vanished
os.env(name str) str?          set (possibly empty) or none
```

The option is the point: `if v := os.env("BIN"); v != none` narrows.
No exit, setenv, hostname until something needs them.

### file growth (spec 15.3 additions)

```
file.glob(pattern str) ([]str, error?)   sorted; empty list for zero
                                         matches; error only for a bad
                                         pattern
file.modified(path str) (int, error?)    epoch nanoseconds
```

Glob semantics: rust glob crate with `require_literal_leading_dot:
true` (`*` does not match dotfiles, shell behavior), `**` crosses
directories, matches files and directories, relative patterns resolve
against the process working directory.

### regex (spec 15.10)

```
regex.compile(pattern str) (Re, error?)
re.matches(s str) bool
re.find(s str) Match?
re.find_all(s str) []Match
re.replace(s str, repl str) str          all occurrences; $1/$name
                                         group references in repl
struct Match { text str, start int, end int, groups []str }
```

`Re` is an opaque handle with reference semantics, the Ctx pattern
(`Value::Re(Arc<regex::Regex>)`, struct type injected with no fields,
methods checked like Ctx's). `start`/`end` are byte offsets into `s`,
half-open. `groups` holds captures 1..n; a group that did not
participate reads `""`. Flavor: the rust regex crate, linear time, no
backreferences or lookaround; the compile error names the missing
feature when a pattern wants one. Full PCRE stays one `import py "re"`
away.

### flag (spec 15.11)

```
struct Flag { name str, short str, fallback str, usage str,
              toggle bool }
flag.parse(flags []Flag) (Parsed, error?)
struct Parsed { values map[str]str, rest []str }
```

Grammar, Go-shaped:

- `--name value`, `--name=value`, `-s value`, `-s=value`.
- A toggle takes no value and parses to `"true"` (`--verbose`); its
  fallback should be `"false"`. `--name=value` on a toggle is an error.
- Parsing stops at `--` (consumed) or at the first argument that does
  not start with `-`; everything after lands in `rest` verbatim. A
  bare `-` is an argument, not a flag.
- Unparsed flags read as their fallback in `values`, so lookups never
  miss: `parsed.values["addr"]`.
- `-h`/`--help` are synthesized; declaring either name is a parse-time
  error (the programmer's, reported as an error value). Help comes
  back as an error whose msg is the rendered usage text: main decides
  where it goes and returns it or prints it. The module never writes
  output and never exits (ADR 0012 applied to CLI UX).
- Unknown flag: error carrying the usage text plus the offending flag.
- Values are strings; `int(x)`/`float(x)` conversions are the typed
  layer and already mandatory-checked.
- `flag.parse` reads `args()` (14.5); tests pass args via the runner's
  existing argv plumbing in goldens (`nv prog.nv -- ...` semantics are
  the program's own args already).

## Integration map (per module)

- `src/typecheck/sigs.rs`: STD_MODULES entry + `std_member` rows;
  struct injection in `collect` for time (Parts), regex (Re, Match),
  flag (Flag, Parsed), following the http `struct_types()` pattern.
- `src/typecheck/expr.rs`: method table entries for `Re` next to the
  Ctx arm.
- `src/value.rs`: `Value::Re(Arc<regex::Regex>)`; display and clone by
  kind (reference copy) per ADR 0010.
- `src/stdlib/{time,os,regex,flag}.rs` + `mod.rs` dispatch + constant
  table entries for the time constants.
- `Cargo.toml`: `regex`, `glob` crates. Both compile to wasm.
- Spec sections 15.8 through 15.11, file additions in 15.3, ctx edit
  in 15.4; the book's stdlib chapter regenerates from the spec at
  build (nothing remembered).
- Goldens per module under `tests/golden/stdlib/`; py-independent, no
  `NEVLA_TEST_PY` gate.

## Golden test sketch

- time: constants ratios, now monotone vs parts invariants, sleep(0),
  sleep under an expired ctx returns the ctx error.
- ctx: existing timeout goldens move to `30 * time.second` forms; a
  zero-duration timeout is immediately done (kept).
- os: workdir is absolute and exists; env round-trip via the runner's
  environment; missing var is none and narrows.
- file: glob against a fixture tree the test writes itself (mkdir +
  write, then glob, then remove); modified moves forward after a
  write.
- regex: compile error on backreference names the feature; find/groups
  offsets; replace with `$1`; find on no match is none and narrows.
- flag: every grammar row above, help as error, unknown flag, toggle,
  `--` handoff, fallback fill-in.
