# Control flow

## if and else

Conditions are plain `bool` expressions, no parentheses, braces always.
`else` and `else if` sit on the same line as the closing brace of the
previous block:

```rikki
fn describe(n int) str {
    if n < 0 {
        return "negative"
    } else if n == 0 {
        return "zero"
    } else {
        return "positive"
    }
}

fn main() {
    print(describe(-3), describe(0), describe(7))   // negative zero positive
}
```

Conditions narrow option types inside the branches; that mechanism has
[its own chapter](options.md).

## for: one loop keyword, three forms

```rikki
fn main() {
    // forever: only break or return leaves it
    n := 0
    for {
        n = n + 1
        if n == 3 {
            break
        }
    }

    // while: condition checked before each round
    for n > 0 {
        n = n - 1
    }

    // range: over an int, list, map, str, or py value
    total := 0
    for i := range 5 {
        total = total + i
    }
    print(total)            // 10
}
```

`range` follows Go, including ranging over an integer:

| operand | one variable | two variables |
|---------|--------------|---------------|
| `int` n | `i` from 0 through n-1 | compile error |
| `[]T` | index | index, element |
| `map[K]V` | key | key, value |
| `str` | index | index, character (a one-char `str`) |
| `py` | iteration index | index, item |

The variables can be dropped entirely (`for range 3 { ... }` runs the
body three times), and `_` discards one position
(`for _, v := range xs`). Maps range in insertion order, always.
Iteration variables are fresh bindings each round, so closures created
in different rounds capture different variables.

```rikki
fn main() {
    for _, c := range "abc" {
        print(c.upper())
    }
    // a map ranges in insertion order
    m := map[str]int{"one": 1, "two": 2}
    for k, v := range m {
        print(k, v)
    }
}
```

## break and continue

`break` leaves the innermost loop; `continue` starts its next round.
Either outside a loop is a compile error. Both count as divergence: the
checker rejects unreachable statements after them, the same way it
rejects code after `return`.

**Coming from Go:** there are no labels, no `goto`, no `switch`, and no
three-clause `for`. `for i := range n` covers counting loops, and an
`if`/`else if` chain covers the rest.
