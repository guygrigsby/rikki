# 20. Factored imports; tidy manages the set

Status: accepted 2026-07-10

## Context

The script stdlib worked: real scripts stopped importing Python. It
also gave every script five or six import lines, and the ceremony read
worse than the imports were worth. Implicit stdlib was considered and
rejected: imports are capability documentation (`import "proc"` tells a
reader this script spawns processes before they read a line), and that
signal is worth keeping explicit. A comma-separated list inside one
string (`import "ctx,flag"`) was also rejected: the path stops being a
path, commas are legal in filenames, and `grep 'import "http"'` stops
finding users of http.

## Decision

Go's answer, both halves:

- The factored import block is syntax: `import ( ... )`, one spec per
  line, `py`-marked specs included, pure sugar for the same sequence of
  single imports. One import form for all three import kinds.
- The gofmt/goimports split is preserved. `nevla fmt` keeps its
  contract (AST preservation, proven by the corpus gate) and only lays
  out: two or more imports render as one factored block in source
  order. `nevla tidy` is the tool allowed to edit: it adds missing
  stdlib imports (module receivers and stdlib-injected struct names
  such as `Parts` or `Cmd` both count as use), removes unused imports
  of every kind, sorts (plain paths, then py, alphabetical within),
  and merges the group at the first import's position, then formats.

Tidy's resolution is syntactic, not scoped; a local shadowing a module
name can fool it, and `nevla check` remains the arbiter. `import
"error"` is never auto-managed: the error constructors need no import
(15.1), so presence is a deliberate statement.

## Consequences

- Scripts carry one import block; the ceremony argument dies without
  losing grep or capability visibility.
- Editors and hooks run tidy where they ran fmt; fmt stays safe to run
  anywhere, tidy is the one that changes meaning-adjacent text.
- The formatter's comment machinery is line-driven, so tidy on an
  import region dense with comments may reflow them (never lose them);
  the ceiling is accepted and revisits with evidence.
