# Options, not nil

This chapter is the biggest departure from Go. There is no nil, no nil
pointer, no nil map, no "invalid memory address" at hour two. Absence
is a type, and the checker makes you look before you touch.

## The option type

For any type `T`, the option type `T?` holds either a `T` or the
absent value `none`. Anywhere a value can be missing, the type says so:
a map read is `V?`, a function that might not find its answer returns
`T?`, a struct field that starts empty is declared `T?`.

```rikki
struct Profile {
    Name str
    Nick str?       // may be absent
}

fn main() {
    p := Profile{Name: "rikki", Nick: none}
    n := p.Nick
    if n != none {
        print(n)
    } else {
        print(p.Name)       // rikki
    }
}
```

A plain `T` widens into `T?` on assignment; the reverse never happens
implicitly. `none` is assignable to every option type and compares only
against options; `none == none` is true.

## Narrowing

An option must be narrowed before use. Using it unnarrowed is a compile
error, not a runtime surprise:

```rikki
fn main() {
    m := map[str]int{"a": 1}
    v := m["a"]             // v: int?
    // print(v + 1)         // compile error: v might be none
    if v != none {
        print(v + 1)        // 2: v is int inside this branch
    }
}
```

The checker follows the control flow (flow typing). A comparison
against `none` narrows in both directions: `if v != none` gives you `v`
as `T` in the then-branch, and `if v == none` gives it to you in the
else-branch.

Early exits narrow the rest of the function. When one side of the
branch always diverges (`return`, `break`, `continue`), the narrowing
survives past the statement:

```rikki
fn first(xs []int) int {
    if len(xs) == 0 {
        return 0
    }
    return xs[0]
}

fn main() {
    m := map[str]int{"a": 1}
    v := m["a"]
    if v == none {
        return
    }
    print(v + 1)            // 2: v is int from here on
    print(first([7]))       // 7
}
```

## Invalidation

Narrowing is erased wherever it can no longer be proven. Assigning to
the variable erases it; a loop body erases it when the body assigns the
variable anywhere, because the body runs more than once. Rebinding with
`:=` makes a new variable and leaves the outer narrowing alone. Losing
a narrowing is always sound; the fix is to re-check or bind the value
out to a new name.

**Coming from Go:** the `v, ok := m[k]` two-value read does not exist.
A map read is one value of type `V?`, and the `ok` check became a type
the compiler enforces. You cannot forget it: code that compiles has
looked.
