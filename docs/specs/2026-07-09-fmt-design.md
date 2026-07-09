# rikki fmt

Date: 2026-07-09. Status: approved. One true style, gofmt's philosophy:
no configuration, whole-file rewrite, idempotent, and the style argument
ends because the tool ends it. Agents amplify the payoff: they match
whatever fmt produces, so lmtk stops drifting.

## Style

- 4-space indents; the grammar already fixes brace placement and
  one-statement-per-line.
- Single spaces around binary operators and `:=`/`=`; space after commas
  and colons; none inside `()` `[]` `{}` delimiters of calls, indexes, and
  literals; none around `.` or inside slice `:`.
- Blank lines: user intent preserved, runs collapsed to one; none allowed
  at the start or end of a block; exactly one between top-level decls,
  except between consecutive imports, which follow the source (import
  groups stay grouped).
- Composite literals (lists, maps, struct literals) and call arguments
  keep the source's break decision: all on one line stays one line, any
  newline inside means one element per line with a trailing comma at
  fmt's indentation. This is the only place source layout is consulted;
  everything else is canonical.
- Comments survive verbatim: own-line comments keep their own line at the
  current indent; trailing comments stay trailing, two spaces before
  `//`. A comment's blank-line neighborhood follows the blank-line rules.

## Architecture

The existing lexer drops comments, so fmt gets a trivia-aware pass:

1. `lexer::lex_trivia(src)` produces the ordinary token stream plus a
   side table of comments (line, column, text, own-line vs trailing) and
   the set of source lines that were blank. The normal lexer is untouched;
   trivia mode is a thin wrapper.
2. The ordinary parser produces the AST (spans already carry lines).
3. A printer walks the AST emitting canonical style, weaving comments in
   by line number: a comment whose line precedes the next node's line is
   emitted own-line before it; one on the same line as the node just
   printed attaches as trailing.

No CST, no parser changes. The cost of this shape is that a comment in a
syntactically weird position (mid-expression) migrates to the nearest
statement boundary; accepted, gofmt does the same.

## Correctness gates

- Semantics preservation, proven over the whole golden corpus: format
  every golden `.rk`, parse both versions, and the ASTs must be equal
  (spans aside); then run the formatted source and stdout must match the
  golden `.out` byte for byte.
- Idempotency: `fmt(fmt(x)) == fmt(x)` over the same corpus.
- Comment preservation: every comment in the input appears in the output
  (count and text), checked over the corpus plus targeted cases.

## Surface

- `rikki fmt [path...]` rewrites files in place (project `src/` when
  bare); `rikki fmt --check` exits nonzero listing unformatted files, for
  CI and the pre-commit gate.
- `fmt_source(src) -> Result<String, Diag>` in the lib, so the playground
  gets a Format button (wasm export) and the nvim plugin can format on
  save later.
- Unparseable source is left untouched and reported; fmt never destroys
  code it cannot understand.
