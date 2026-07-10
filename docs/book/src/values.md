# Values and the copy model

## The types

```
int   float   bool   str
[]T   map[K]V   T?
fn(...) ...   struct types   error   py
```

`int` is 64-bit signed and overflow is a fault, never a silent wrap.
`float` is IEEE 754 double. `str` is an immutable sequence of Unicode
characters ("character" means code point everywhere in rikki). `bool`
has exactly `true` and `false`; there is no truthiness, and conditions
must be `bool`.

`int` and `float` never mix: `1 + 2.5` is a compile error. Convert
explicitly.

## Declaring and assigning

`x := e` declares `x` with the type of `e`; there is no way to declare
a variable with an explicit type, and no top-level variables or
constants. `x = e` assigns to an existing variable, element, or field.
Shadowing in an inner scope is fine; redeclaring in the same scope is a
compile error.

## Conversions

`int(x)`, `float(x)`, `bool(x)`, `str(x)`, and `[]T(x)` are the casts.
Fallibility follows the operand: converting from `str` is a parse and
returns `(T, error?)`; numeric conversions cannot fail and are used
inline.

```rikki
fn main() (error?) {
    n, err := int("42")     // a parse can fail
    if err != none {
        return err
    }
    print(n)                // 42
    i := int(3.9)           // 3: truncation toward zero, single-valued
    s := str(123) + "!"
    print(i, s)             // 3 123!
    return none
}
```

**Coming from Go:** conversion is spelled like a call on the type name,
and the fallible cases return an error value instead of silently
producing a zero. `int("x")` gives you `(0, error)`; nothing panics and
nothing guesses.

## Value types and reference types

Rikki splits its types the way Go does. Scalars, strings, structs, and
errors are value types: assignment, argument passing, and iteration
bind copies. Lists, maps, functions, and `py` values are reference
types: one underlying object, however many names point at it.

```rikki
struct User {
    Name str
    Age  int
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
```

A struct copy is shallow in Go's sense: a reference-typed field copies
the reference, so the copy's containers alias the original's.

The zero value of a list or map is a fresh empty container, immediately
usable. There is no nil and no nil-map write crash.

`clone(xs)` makes an explicit one-level copy of a list or map, matching
Go's `slices.Clone`. `append(xs, v)` is pure: it returns a fresh list
and never mutates in place, so growth is visible only by rebinding
(`xs = append(xs, v)`). The list methods (`map`, `filter`, `sorted`)
also return fresh lists.

## Equality

`==` and `!=` work on scalars (`int`, `float`, `bool`, `str`) and for
comparing an option against `none`. Nothing else compares with `==`;
lists, maps, structs, and errors are compile errors. Structural
equality goes through `contains` or an explicit walk, and tests use
`test.eq`, which compares structurally and reports the difference.

`<`, `<=`, `>`, `>=` order `int`, `float`, and `str` (lexicographic by
character). Chained comparisons (`a < b < c`) are rejected by the
checker.
