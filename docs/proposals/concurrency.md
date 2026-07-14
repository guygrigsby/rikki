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

- 2026-07-10, proc landed (spec 15.12): the pump threads exist as
  designed; only Strings cross them (Mutex<VecDeque<String>> plus a
  Condvar). run/exec/wait/readline all slice-poll (~20ms) against the
  ctx, the time.sleep pattern; three call sites now share that shape,
  which is the evidence a select/park primitive would consolidate.

- 2026-07-10, proc comment review: Ctx is not waitable. It is a
  deadline plus an AtomicBool, so every blocked stdlib call (time.sleep,
  proc wait/readline/run) must wake on a slice to notice cancellation;
  data paths already park on condvars. Giving CtxInner a condvar or
  waker that timeout deadlines and the SIGINT handler signal would let
  blockers park exactly and delete the slice-polling class. Any future
  select primitive needs this anyway: select over (line, exit,
  cancellation) is only as good as its worst event source.

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
- 2026-07-13, bytes design: lent `[]byte` buffers are memory shared with
  Python (always-view bridge crossing, buffer protocol). Safe today only
  because nevla is single-threaded and the GIL serializes Python; any
  parallel design must give lent buffers the same answer as shared lists,
  and they are observable from foreign code, so "don't" is even weaker
  here.
- 2026-07-14, bytes fix round: the lent flag from the 2026-07-13 bytes
  design (buffers that ever crossed the bridge stop growing in place) is
  gone — its only reason to exist, `append`'s in-place-growth fast path,
  was itself removed for breaking `append`'s purity contract (no
  refcount threshold can tell "the caller is about to rebind this name" from
  "the caller is keeping this name and binding the result elsewhere";
  `[]byte` append now always copies, like every other `[]T`). Task 7's
  buffer-protocol bridge crossing no longer has a private/shared
  distinction to lean on; whichever future synchronization or
  freeze-on-share story lands still needs its own seam, not this one.
- 2026-07-14, file handles (task 6, spec 15.3): `File` joins Proc and
  Ctx as an Arc-backed opaque handle with a Mutex'd interior
  (`Mutex<Option<fs::File>>`; Arc+Mutex where ordinary values are
  Rc+RefCell). Single-threaded today, so the lock never contends; the
  shape means a handle can cross whatever task boundary the eventual
  concurrency model draws without changing the user-visible type. Same
  wrappability constraint as Proc's blocking reads: `f.read(n)` is a
  blocking pull a channel or select design must be able to absorb.
- 2026-07-14, task 7 review: `bytesview` (the `[]byte` bridge view) is an
  `unsendable` pyclass holding an `Rc`; pyo3 panics (a caught
  PanicException at dealloc, process exit unaffected) if one is
  deallocated on a thread other than its creator.
  `bridge::release_pending()` covers the deferred-decref-queue vector:
  values dropped while the thread is detached flush on the run's own
  interp thread before it exits. It structurally cannot cover a view
  stashed into a cross-run-persistent python global (`builtins.__dict__`,
  a module attribute) in run 1 and deallocated by run 2's thread — only
  reachable in multi-run-per-process embedding with deliberate stashing;
  the CLI (one run per process) is immune. No code fix without forfeiting
  the design: `Rc` cannot be `Send`, and the sendable alternatives give
  up zero-copy. Whatever threading story lands must either pin py
  teardown to an owner thread or make the buffer's cell `Send`; this
  hazard is its concrete test case.
