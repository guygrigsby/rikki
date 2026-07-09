# Exported-identifier visibility

Date: 2026-07-09. Status: approved. Source: packages discussion; the Go
rule is muscle memory. Breaking (file modules currently expose
everything), shipped now per the break-early posture, before anything
depends on the old shape. Prerequisite for packages (backlog v2).

## The rule

A top-level name is exported when its first character is an ASCII capital;
otherwise it is private to its file module. No keywords. Applies to:

- functions: `util.Double(...)` works from an importer, `util.twice(...)`
  is "twice is not exported by util";
- struct types: a lowercase struct cannot be named in a foreign struct
  literal ("pair is not exported by util");
- struct fields: a foreign field read of a lowercase field is
  "field secret of util.Pair is not exported", and a struct with any
  unexported field cannot be constructed by a foreign literal at all
  (literals require every field; construct it behind an exported
  function, Go's constructor pattern).

Inside the defining file nothing changes: lowercase helpers call
lowercase helpers, and a file constructs its own structs freely. An
exported function may return an unexported struct; the value flows and
its exported fields read fine, the importer just cannot name the type in
a literal or touch its unexported fields.

Out of scope: the python stdlib bridge and rikki's own stdlib modules
(their members are defined by spec chapter 15; `math.sqrt` stays
lowercase), py values (Python's rules), and local names inside functions.
The repl is unchecked (17.6), so visibility is a compile-time rule like
every other check.

## Implementation

Checker only; the loader's qualified-name flattening already makes every
cross-module access syntactically distinct (module member paths and
dotted struct names), while same-module references were renamed to bare
qualified identifiers and bypass the checks naturally.

- Module member call (`ImportKind::File`): a resolvable member with a
  lowercase name diagnoses "not exported"; an unresolvable one keeps
  "has no member".
- Field access on `Type::Struct("m.S")`: foreign when `m` differs from
  the checking file's stem; a defined lowercase field diagnoses "not
  exported".
- Dotted struct literal `m.S{...}`: `S` must be exported; a foreign
  struct with any unexported field diagnoses once and skips field
  checking.

## Spec changes

Chapter 16 gains the visibility rule (the normative home); 6.3 and 7.2.3
cross-reference it; chapter 15 notes stdlib exemption.

## Verification

Goldens: imports/basic and imports/struct-lit flip to exported names and
keep passing (with an internal lowercase call proving same-file use); a
new imports/unexported case pins all three diagnostics. The full gate
sweeps for other cross-module lowercase uses.
