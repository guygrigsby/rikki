#!/usr/bin/env python3
"""mdBook preprocessor: every complete-program ```rikki block gains a
playground link with the program base64url-encoded into the URL fragment,
so the link and the code cannot drift apart. Fragments (no fn main) are
left alone. tests/book.rs keeps the blocks themselves compiling."""

import base64
import json
import re
import sys

PLAYGROUND = "https://rikki.aeryx.ai/#"
BLOCK = re.compile(r"```rikki\n(.*?)```\n", re.S)


def link(code: str) -> str:
    b = base64.urlsafe_b64encode(code.encode()).decode().rstrip("=")
    return f"\n[**▸ run it in the playground**]({PLAYGROUND}{b})\n"


def process(content: str) -> str:
    def repl(m: "re.Match[str]") -> str:
        code = m.group(1)
        if "fn main" in code:
            return m.group(0) + link(code)
        return m.group(0)

    return BLOCK.sub(repl, content)


def walk(items) -> None:
    for it in items:
        ch = it.get("Chapter")
        if ch:
            ch["content"] = process(ch["content"])
            walk(ch.get("sub_items", []))


if len(sys.argv) > 1 and sys.argv[1] == "supports":
    sys.exit(0)
_ctx, book = json.load(sys.stdin)
walk(book.get("sections") or book.get("items") or [])
print(json.dumps(book))
