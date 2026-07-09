import init, { run, version } from "./pkg/rikki_playground.js";

const EXAMPLES = {
  "hello world": `fn main() {
    print("hello, rikki")
}
`,
  "errors are values": `fn half(n int) (int, error?) {
    if n % 2 != 0 {
        return 0, error.new("odd number")
    }
    return n / 2, none
}

fn main() {
    v, err := half(42)
    if err != none {
        print("half(42) came back with an error: " + err.msg)
    } else {
        print(v)
    }
    _, err2 := half(7)
    if err2 != none {
        print("half(7) handled cleanly: " + err2.msg)
    }
}
`,
  "the copy model": `struct User {
    Name str
    Age int
}

fn main() {
    u := User{Name: "rikki", Age: 1}
    v := u
    v.Age = 99
    print(u.Age)    // 1: structs copy

    xs := [1, 2, 3]
    ys := xs
    ys[0] = 99
    print(xs[0])    // 99: lists are references
}
`,
  "options, no nil": `fn main() {
    m := map[str]int{"a": 1}
    v := m["a"]
    if v != none {
        print(v + 1)
    }
    if m["zzz"] == none {
        print("no zzz in the map, and the checker made us look")
    }
}
`,
  "py bridge (native only)": `// the bridge needs a real CPython; install rikki
// (uv tool install rikki-lang) to run this one
import py "json"

fn main() (error?) {
    x := check json.loads("[1, 2, 3]")
    xs := check []int(x)
    print(xs.sum())
    return none
}
`,
};

const editor = document.getElementById("editor");
const gutter = document.getElementById("gutter");
const output = document.getElementById("output");
const status = document.getElementById("status");
const examples = document.getElementById("examples");

let errLines = new Set();

function renderGutter() {
  const n = editor.value.split("\n").length;
  gutter.replaceChildren();
  for (let i = 1; i <= n; i++) {
    const d = document.createElement("div");
    d.textContent = i;
    if (errLines.has(i)) d.className = "err-line";
    gutter.appendChild(d);
  }
  gutter.scrollTop = editor.scrollTop;
}

for (const name of Object.keys(EXAMPLES)) {
  const o = document.createElement("option");
  o.value = name;
  o.textContent = name;
  examples.appendChild(o);
}

function execute() {
  const r = run(editor.value);
  errLines = new Set();
  if (r.status === "ok") {
    status.textContent = "ok \u00b7 program output";
    status.classList.remove("error");
    output.classList.remove("error");
    output.textContent = r.stdout.length ? r.stdout : "(no output)";
  } else {
    status.textContent = r.status === "compile" ? "compile error" : "runtime error";
    status.classList.add("error");
    output.classList.add("error");
    output.textContent = (r.stdout.length ? r.stdout + "\n" : "") + r.error;
    // diagnostics lead with line:col; light the gutter up
    for (const m of r.error.matchAll(/^(\d+):\d+:/gm)) {
      errLines.add(Number(m[1]));
    }
  }
  renderGutter();
}

function share() {
  const bytes = new TextEncoder().encode(editor.value);
  const b64 = btoa(String.fromCharCode(...bytes))
    .replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
  location.hash = b64;
  navigator.clipboard?.writeText(location.href);
  status.textContent = "link copied";
  status.classList.remove("error");
}

function fromHash() {
  if (location.hash.length < 2) return false;
  try {
    const b64 = location.hash.slice(1).replaceAll("-", "+").replaceAll("_", "/");
    const bin = atob(b64);
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    editor.value = new TextDecoder().decode(bytes);
    return true;
  } catch {
    return false;
  }
}

document.getElementById("run").addEventListener("click", execute);
document.getElementById("share").addEventListener("click", share);
examples.addEventListener("change", () => {
  editor.value = EXAMPLES[examples.value];
  history.replaceState(null, "", location.pathname);
  execute();
});
editor.addEventListener("input", () => {
  errLines = new Set();
  renderGutter();
});
editor.addEventListener("scroll", () => {
  gutter.scrollTop = editor.scrollTop;
});
editor.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
    e.preventDefault();
    execute();
  } else if (e.key === "Tab") {
    e.preventDefault();
    const { selectionStart: s, selectionEnd: t } = editor;
    editor.setRangeText("    ", s, t, "end");
  }
});

await init();
document.getElementById("version").textContent = `rikki ${version()} · wasm`;
if (!fromHash()) {
  editor.value = EXAMPLES["hello world"];
}
execute();
