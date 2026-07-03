# nvim editor support: syntax highlighting + check-on-save

Date: 2026-07-02. Status: approved.

## Why

Editing .rk in nvim is currently colorless and errors only surface on a
manual `rikki check`. LSP and tree-sitter were both considered and
deliberately deferred: the syntax is still moving fast, a tree-sitter
grammar is a second parser that drifts (plus rikki's significant newlines
and composite-literal ambiguity likely force an external scanner), and an
LSP's value (semantics from the real checker) is worth building only once
the surface settles. The minimal pair that works today: a vim syntax file
for colors, `rikki check` wired into vim.diagnostic for squiggles.

## Shape

`editors/nvim/` is an nvim runtime directory, usable via any plugin manager
pointed at the path or a runtimepath append:

- `ftdetect/rikki.vim`: `*.rk` and `#!...tk` shebangs set `filetype=rikki`.
- `syntax/rikki.vim`: lexical classes only, grounded in lexer.rs:
  keywords (`fn struct import py return if else for range break continue
  check`), `true false` / `none`, type names (`int float bool str error
  py map`), builtins (`print printf sprintf len append ord chr args
  input`), `//` comments, one-line strings with exactly the lexer's four
  escapes (`\n \t \" \\`), integers and simple floats (no e-notation; the
  lexer has none), operators including `:=` and `@`, and fn-declaration
  names.
- `ftplugin/rikki.lua`: `commentstring = "// %s"`; on `BufWritePost` (and
  once on load) run `{rikki_bin} check <file>` async via `vim.system`,
  parse `file:line:col: msg` lines into `vim.diagnostic` ERROR entries on
  the buffer. Lines naming other files (imports) or without a span
  (loader errors) attach at line 1 so nothing is silently dropped. Binary
  is `rikki` from PATH; `vim.g.rikki_bin` overrides. A clean check clears
  the diagnostics.
- `README.md`: install one-liner for lazy.nvim and plain runtimepath.

## Non-goals

No LSP, no tree-sitter, no indent engine, no semantic highlighting. Colors
are lexical classes only, so the files stay cheap to keep current as the
language changes.

## Verification

No automated harness gains an nvim dependency. Verified by driving nvim
headless: open a file with a known checker error, write it, assert the
diagnostic appears at the right position; then a clean file clears it.
Highlighting is eyeballed against lmtk's sources, the largest real corpus.
