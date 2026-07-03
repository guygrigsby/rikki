# rikki nvim support

Syntax highlighting for `.rk` plus `rikki check` on save wired into
`vim.diagnostic` (squiggles, `vim.diagnostic.setloclist()` for the list).
Needs nvim 0.10+ and `rikki` on PATH (`vim.g.rikki_bin` overrides).

lazy.nvim:

```lua
{ dir = "~/projects/rikki/editors/nvim" }
```

or plain runtimepath:

```lua
vim.opt.rtp:append(vim.fn.expand("~/projects/rikki/editors/nvim"))
```

Deliberately not an LSP or a tree-sitter grammar while the language is
still moving; see docs/specs/2026-07-02-nvim-editor-support-design.md.
