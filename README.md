# SupervisorMachines 🔁

> **OTP-style supervision trees over Ruby threads.** The command vessel that
> notices a station has gone quiet and decides who returns to duty.

A child died. Which children restart, and has that happened too often to keep
trying? Those answers come from the `no_std` [supervisor-machines][core] crate.
Everything the core refuses to touch, spawning, joining, killing, ordered
shutdown, is this gem. **AMS Armstrong** is the bridge around that decision.

---

## 🚀 Launch Sequence

```bash
# Add to your ship's Gemfile
gem 'supervisor_machines'
```

Requires **Ruby 4.0+** and a Rust toolchain, or a platform gem: the core is a
native extension, not an option.

---

## 🎯 Mission Brief

```ruby
SM = SupervisorMachines

SECOND = 1_000_000_000

tree = SM::Supervisor.new(:one_for_all, SM::Intensity.new(3, 60 * SECOND))
tree.supervise(:cache)  { Thread.new { cache.run } }
tree.supervise(:warmup, policy: :transient) { Thread.new { warm_the_cache } }
tree.start

tree.children    # => [:cache, :warmup]
tree.restarts    # => 0
tree.terminated? # => false

tree.stop(5 * SECOND)
```

A starter block returns the `Thread` that runs the child, and is called again
for every restart. A child that stops is noticed, handed to the core, and
whatever the core decides is carried out.

Every instant and span is **Integer nanoseconds**, in and out: a non-Integer
raises `TypeError`, a negative one `RangeError`. `stop(grace)` takes a grace in
nanoseconds or `nil` for no limit; `Supervisor::ESCALATION_GRACE` is
nanoseconds too. The sibling gems the tree drives (chrono_machines,
deadline_machines) still speak Float seconds; the tree converts at their door.

---

## 🔄 Lifecycle

A tree registers children, runs them, and ends either stopped or failed. Both
endings are final: `start` raises `SupervisorMachines::Error` for a tree that
is already running, has been stopped, or gave up. There is one generation of
children per tree, and starting a second one is a bug, not a refresh.
`supervise` is refused the same way once `start` has fixed the shape of the
tree: a tree past registering cannot take on a child.

A `start` that cannot finish, because a starter raises, takes the children it
already brought up back down in reverse start order, leaves the tree failed,
and raises what the starter raised. Nothing is left running and nothing is left
unsupervised. `stop` then raises `Escalated`, like any other tree that gave up,
every time it is called, and a tree that was never started has nothing to take
down.

---

## 🎛️ Strategies

`:one_for_one` restarts only what died. `:one_for_all` restarts every child,
for a set that only works together. `:rest_for_one` restarts the child and
everything started after it, for a chain where later children depend on earlier
ones.

One restart is recorded per failure handled, not per child brought back, so the
strategy does not change how fast the restart intensity is spent. That is OTP's
accounting.

---

## 📋 Standing Orders

A child's policy is its standing order. `:permanent` always comes back, even
from a clean exit. `:transient` comes back
only from a bad one, because a clean exit means it finished. `:temporary` never
comes back, and never spends the restart budget.

```ruby
tree.supervise(:migration, policy: :temporary) { Thread.new { migrate } }
```

---

## 🔥 Restart Intensity

An `SupervisorMachines::Intensity.new(restarts, window)`, `restarts` within
`window` nanoseconds, is all the thrashing a tree will do.
`Intensity.default` is OTP's one restart in five seconds. Past
it, the tree stops its remaining children in reverse start order and raises
`SupervisorMachines::Escalated`, and the supervisor's own supervisor now owns
the problem. That decision is sticky: `terminated?` stays true and every later
`stop` raises again.

---

## 🚫 Rules of Engagement

- The delay between a restart decision and the respawn comes from
  [chrono_machines][chrono]: `Supervisor::BACKOFF`, 100 ms doubling to 10 s
  with full jitter. Pass another shape as `backoff:` to change the curve.
- The shutdown budget comes from [deadline_machines][deadline]: `stop(grace)`
  bounds the whole ordered teardown, and a child that outstays it is left
  behind rather than waited on forever.
- Restart intensity, strategy spans and policy decisions come from the core
  crate. None of it is reimplemented here.

---

## 📐 Shape

The core carries its child count in its type, so a tree is dispatched to one of
`SupervisorMachines::Decider::SUPPORTED_CHILDREN` monomorphised shapes: 1
through 16, and 32. Any other count raises `RangeError` from `#start`. More
restarts than `Decider::MAX_RESTARTS` (the 64-instant history minus one) raise
`RangeError` from `Supervisor.new`. An unknown strategy or policy symbol is a
`RangeError`, a non-Symbol a `TypeError`.

Threads are the only supervised unit.

`Supervisor` is a runtime on top of the core, not a mirror of it, so it keeps
two keyword defaults the core has no counterpart for: `backoff:` (a
chrono_machines shape, asked `delay(restarts, draw)` in nanoseconds) and
`supervise(name, policy: :permanent)`, OTP's default child policy. The
`Decider` below is the mirror, and takes no keyword at all.

---

## 🧭 The Decider

`SupervisorMachines::Decider` is the core's `Supervisor` itself, for callers
who run their own processes, fibers or reactors. It mirrors the Rust API
method for method, positionally and in nanoseconds; refusals are symbols and
never raise.

```ruby
decider = SM::Decider.new(3, :rest_for_one, SM::Intensity.new(2, 60 * SECOND))
decider.stopped(1, :permanent, :abnormal, now)
# => #<data SupervisorMachines::Restart span=#<data SupervisorMachines::Span first=1, last=2>>
decider.stopped(7, :permanent, :abnormal, now) # => :unknown_child
```

Values are `Data`, validated through the core whether the gem built them or
you did: `Intensity(restarts, window)` (zero window: `RangeError`),
`Span(first, last)` (`first` past `last`: `RangeError`), `Restart(span)`.
They are final: subclassing one raises `TypeError` and `allocate` is gone.
Sealing stops accidents, not reflection (a named subclass's constant is bound
before `inherited` raises, and `Marshal.load` skips `initialize`), so the
binding is the authority: every value it reads must be an instance of exactly
the gem's class (`TypeError` otherwise, a subclass included) and is rebuilt
through the core constructor (`RangeError` for a forged one). `Decider` wraps
native state with no allocator, and is sealed too: subclassing it raises
`TypeError`.

### Contract

- **One refusal**, `:unknown_child`: an index at or past the child count. It is
  checked first, on a terminated decider too, and changes nothing.
- **Honoured repeat.** Once escalated, every further `stopped` answers
  `:already_escalated` and records nothing.
- **The clock.** The decider keeps no clock but its restart history. Answers
  that depend on `now` record it (a `Restart`, `:escalate`, `:recorded`);
  answers that hold whatever the time (`:ignore`, `:already_escalated`,
  `:outdated`) record nothing. Restarts may arrive in any order.
- **No scopes.** Child indices are your own numbers; nothing is issued.
  `Decider.restore` rebuilds from a saved `history`, every instant counted
  (two restarts at one instant restore as two); escalation is not saved, so a
  restored decider decides for itself. `absorb` merges an Array of Integer
  nanoseconds (any order) keeping the larger count of each instant, so
  another process's two restarts at one instant stay two and a repeated
  absorb changes nothing.
- **Malformed input raises** before the core is touched: `TypeError` for the
  wrong type, `RangeError` for a value outside what the core accepts,
  `FrozenError` for a frozen decider.
- **Never loses work.** `stopped` decides on a copy and keeps it only once the
  answer is built: a raise while building a `Restart` (a redefined `new`)
  changes nothing, freezing the decider meanwhile raises `FrozenError` and a
  call that changed it `RuntimeError`, with the outer `stopped` not applied.
  No reader holds the decider while it builds a `Data` answer, so a `new` that
  calls back into it makes an ordinary call.

### Every answer

| Method | Answers | Records the clock |
| --- | --- | --- |
| `Decider.new(children, strategy, intensity)` | a decider; `RangeError` for an unsupported count or too many restarts | - |
| `Decider.restore(children, strategy, intensity, history, now)` | a decider holding the restarts inside the window as of `now` | - |
| `stopped(index, policy, exit, now)` | `:ignore` | no |
| | `Restart(span)`: one restart recorded at `now` | yes |
| | `:escalate`: recorded at `now`, now terminated | yes |
| | `:already_escalated`: terminated before the call | no |
| | `:unknown_child` (refusal): nothing changed | no |
| `record_external_restart(now)` | `:recorded` | yes |
| | `:outdated`: a full history of newer restarts, nothing changed | no |
| `absorb(history, now)` | Integer, restarts added (larger count of each instant kept); `0` changes nothing | only the added instants |
| `recent_restarts(now)` | Integer | no |
| `restart_delay(base, ceiling, now)` | Integer nanoseconds | no |
| `span(index)` | `Span`, or `nil` past the child count | no |
| `history` | frozen Array of Integer nanoseconds, oldest first | no |
| `strategy`, `intensity`, `children`, `terminated?` | readers | no |

---

## 📡 Credits

**AMS Armstrong**, NCC-3186, *Armstrong*-class command vessel.

Named for Joe Armstrong's OTP supervision trees, which settled that the way to
keep a system up is to let a part of it die and have something above it decide
what comes back.

---

## License

MIT.

[core]: ext/supervisor_machines_native/core/README.md
[chrono]: https://github.com/seuros/chrono_machines
[deadline]: https://github.com/seuros/deadline_machines
