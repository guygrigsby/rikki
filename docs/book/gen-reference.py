#!/usr/bin/env python3
"""Generate the book's reference chapters from the language spec, the
single source of truth for builtins and stdlib (nothing-remembered rule:
the spec is maintained under the same-commit discipline; the book renders
it). Output files are gitignored and rebuilt on every book build."""

import pathlib
import re

HERE = pathlib.Path(__file__).parent
SPEC = HERE / "../../language-spec.md"

BANNER = (
    "<!-- GENERATED from language-spec.md by gen-reference.py; do not edit -->\n"
    "> Generated from [the language spec]"
    "(https://github.com/guygrigsby/rikki/blob/main/language-spec.md), "
    "which is normative.\n\n"
)


def slice_chapter(text: str, start: str, end: str) -> str:
    m = re.search(re.escape(start) + r"\n(.*?)\n" + re.escape(end), text, re.S)
    if not m:
        raise SystemExit(f"spec slice not found: {start!r}")
    return m.group(1).strip() + "\n"


def main() -> None:
    text = SPEC.read_text()
    chapters = [
        ("builtins.md", "# Builtins", "## 14. Builtin functions", "## 15."),
        ("stdlib.md", "# Standard library", "## 15. Standard library", "## 16."),
    ]
    for fname, title, start, end in chapters:
        body = slice_chapter(text, start, end)
        # spec subsections (### 14.1 print) become book sections (## print)
        body = re.sub(r"^### \d+\.\d+ ", "## ", body, flags=re.M)
        out = HERE / "src" / fname
        out.write_text(f"{title}\n\n{BANNER}{body}")
        print(f"wrote {out}")


if __name__ == "__main__":
    main()
