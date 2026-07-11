# 18. Identifier case: Rust convention, Go visibility

Status: accepted 2026-07-10

## Context

The corpus had committed to a case system without recording it: stdlib
methods are snake_case (`starts_with`, `trim_prefix`, `sorted_by`),
types are UpperCamelCase (`User`, `Parts`, `Ctx`), the multi-word
functions in the golden tests are snake (`parse_age`), visibility is
Go's first-capital rule (16.3), and no camelCase exists anywhere. The
question surfaced when the spec gained its first multi-word local
(`elapsed_ms`). Full Go MixedCaps would have required renaming the
entire method surface to avoid a mixed read.

## Decision

Rust's convention with Go's visibility:

- Types are UpperCamelCase: `User`, `Parts`, `TrainRun`.
- Everything else, functions, methods, locals, fields, module names,
  is snake_case: `parse_age`, `has_prefix`, `elapsed_ms`.
- Visibility stays Go's first-capital rule, so an exported multi-word
  function or field capitalizes as UpperCamelCase: `ParseAge` exported,
  `parse_age` private. Exported names read like types; that is the
  accepted wart, priced against renaming the whole surface.

Convention, not enforcement: the checker does not reject other cases.
`nevla fmt` may warn someday; it does not rewrite names.

## Consequences

- Zero renames; every existing name already conforms.
- Spec examples and docs use the convention everywhere, so new code
  copies it by imitation.
- The book gains a style note; contributors stop guessing.
