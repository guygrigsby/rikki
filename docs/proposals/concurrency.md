# Concurrency ledger

A running list of every concurrency consideration we hit while building,
so the eventual design (its own ADR, not coming soon) starts from
evidence instead of memory. Append entries with a date and where the
consideration came from; never delete, strike through with a note if
one becomes moot. Sibling docs in docs/proposals/ are working proposals:
drafts that are not yet decisions and may never become one.

## Constraints (things any design must survive)

- 2026-07-10, proc ADR 0016: `Value` is Rc/RefCell; nothing is Send.
  Runtime threads exist inside the stdlib (pipe pumps), and the
  load-bearing invariant is that no Value ever crosses a thread. A
  user-level design either makes values Send (Arc everywhere, cost on
  every program), isolates heaps per task (actor-shaped), or serializes
  at the boundary.
- 2026-07-10, proc ADR 0016: the GIL is a global choke point. Parallel
  nevla code that touches the py bridge serializes on CPython no matter
  what the nevla side does. A design that promises parallelism must say
  what happens when both sides of a fork hold py values.
- 2026-07-10, copy model (ADR 0010): lists, maps, fn, py, and ctx are
  shared reference cells. Two tasks mutating one list is the exact race
  Go accepts and documents; nevla's no-crash guarantee cannot accept a
  segfault, so shared mutation needs an answer stronger than "don't".
- 2026-07-10, ctx: Ctx is already a cancellation tree (background,
  timeout, interrupt) and every blocking stdlib call takes one. Any
  future task/goroutine gets its lifetime from a Ctx or the two systems
  fight; structured concurrency falls out almost for free if tasks are
  born from a Ctx.

## Shapes to keep wrappable (API decided now, absorbable later)

- 2026-07-10, proc ADR 0016: `Proc.readline(c Ctx)` and `wait(c Ctx)`
  are blocking reads on a runtime-buffered stream. A channel design
  should be able to wrap them (readline becomes a receive) without
  breaking existing code. proc must not grow API a channel world could
  not absorb.
- 2026-07-10, http: `http.stream` takes a per-line callback because
  pre-ADR-0010 closures could not accumulate. It is the only
  callback-shaped API in the stdlib; a stream/channel design should
  subsume it and the callback form can then deprecate.
- 2026-07-10, time (ADR 0015): `time.sleep(c Ctx, secs)` wakes early on
  ctx end. Sleep is already a select of deadline vs cancellation; a
  select primitive would generalize it.

## Users waiting on this (evidence of demand)

- 2026-07-10, dev-watch example: wants to watch the filesystem, pump a
  child's output, and honor SIGINT at once. Today the runtime threads
  hide two of the three; a poll loop covers the rest at 2s latency.
- 2026-07-10, httpcheck example: probes endpoints serially. The obvious
  improvement is N concurrent probes with a bounded fan-out; this is
  the canonical worker-pool shape and would be the first user of any
  spawn primitive.
- 2026-07-10, engineering rules: parallelize-everything is a house
  rule; the language its owner uses daily cannot stay serial forever.
  ML workloads (the founding use case) want concurrent data loading
  next to GPU work.

## Open questions

- Goroutines+channels wholesale (ADR 0013 default) vs structured tasks
  under Ctx vs async/await. Go's model assumes shared memory; the copy
  model's reference types make that the hard part, not the syntax.
- What does `select` mean when one arm is a py operation that cannot be
  cancelled mid-call (the GIL holds it)?
- Test runner parallelism: tests are fallible functions (ADR 0012), an
  embarrassingly parallel list, and likely the first internal user of
  whatever primitive lands.
