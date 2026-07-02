# 8. Go syntax for map types and range loops

Status: accepted

## Context

v1 shipped a mixed idiom: Go-shaped blocks and declarations, but Python-shaped
iteration (`for x in xs`, a `range(n)` builtin returning `[]int`) and a map
type written `map[K, V]`. The language's stated discipline is Go's; the
iteration surface read as neither language and was the first thing that felt
foreign coming from either side.

## Decision

Adopt Go's forms outright.

- Map types are written `map[K]V`; the comma form is gone. Literals follow:
  `map[str]int{"a": 1}`.
- `for` adopts Go 1.22 range semantics as the only iteration form:
  `for i := range n` (0 through n-1, zero iterations for n <= 0),
  `for i := range xs` (indices), `for i, v := range xs`,
  `for k := range m`, `for k, v := range m` (insertion order), and bare
  `for range e`. The condition and infinite forms are unchanged.
- `range` becomes a reserved keyword; the `range()` builtin and the `in`
  keyword are removed.

## Consequences

- Single-variable list loops now bind the index, not the element; element
  iteration is `for _, v := range xs`. This silently changes the meaning of
  any old-style loop that survives textually, but the old syntax no longer
  parses, so nothing survives textually.
- `range(a, b)` had no Go analogue and is gone; write `for i := range b - a`
  and offset, or range a literal list.
- The `range(n).map(f)` idiom for building a list of n things is gone, and
  with `[]` requiring a typed context there is no clean way to build a list
  from nothing; the seed-element-then-append workaround in
  `examples/similarity` marks this as real friction. An empty typed list
  literal (`[]T{}`) is a candidate fix, tracked in the backlog.
