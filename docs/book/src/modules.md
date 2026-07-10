# Modules

## File imports

`import "util.rk"` binds a sibling source file as module `util`. The
path is relative to the importing file, the module's name is the file
stem, and its members are reached with the dot: `util.Double(3)`,
`util.Pair{...}`. Diamond imports load once; an import cycle is a
compile error naming the cycle.

```rikki
// util.rk
fn Double(n int) int {
    return n * 2
}
```

```
// main.rk
import "util.rk"

fn main() {
    print(util.Double(21))      // 42
}
```

Modules are namespaces, not values: `u := util` does not compile, and
module functions are called directly.

## Visibility: the capital letter

A module's top-level name is exported when it starts with a capital
letter; otherwise it is private to its file. No keywords, Go's rule,
and it applies to struct fields too:

- calling an unexported function through a module is a compile error;
- a struct with any unexported field cannot be constructed outside its
  module at all, because literals must supply every field. That is the
  constructor pattern: keep one field lowercase and exports control
  creation.

Inside the defining file, everything is reachable; the rule binds at
module boundaries only.

## Test files are inside the boundary

A file named `util_test.rk` sees `util.rk`'s unexported names through
the ordinary qualified syntax, exactly like Go's same-package tests.
The [testing chapter](testing.md) covers the rest.

## The three imports

```
import "file"           // standard library (math, error, file, ctx, gpu, http, test)
import "util.rk"        // another rikki file, namespaced by stem
import py "torch"       // a Python module through the bridge
```

Standard library modules are documented in
[the reference](stdlib.md). Py imports are governed by the project
manifest: inside a project, `import py` of an undeclared package is a
compile error ([the py bridge](py.md)).
