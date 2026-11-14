# supervisor-machines

Supervision decisions, in the OTP sense. `no_std`, allocation-free, zero
dependencies, no features.

A child died. Which children restart, and has that happened too often to keep
trying? That is all this crate answers. It never spawns, kills or waits for
anything, so it works the same whether the caller supervises threads, fibers or
processes.

```rust
use core::time::Duration;
use supervisor_machines::{Decision, Exit, Intensity, Policy, Span, Strategy, Supervisor};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let seconds = Duration::from_secs;

// Three children that only work as a set, two restarts per minute allowed.
let intensity = Intensity::new(2, seconds(60)).ok_or("zero window")?;
let mut tree: Supervisor<3, 8> = Supervisor::new(Strategy::OneForAll, intensity).ok_or("shape")?;

// The middle child dies badly, so all three come back.
let all = Span::new(0, 2).ok_or("span")?;
assert_eq!(
    tree.stopped(1, Policy::Permanent, Exit::Abnormal, seconds(0)),
    Ok(Decision::Restart(all)),
);

// Keep failing and the supervisor gives up rather than thrashing.
tree.stopped(1, Policy::Permanent, Exit::Abnormal, seconds(1))?;
assert_eq!(
    tree.stopped(1, Policy::Permanent, Exit::Abnormal, seconds(2)),
    Ok(Decision::Escalate),
);
assert!(tree.terminated());
# Ok(())
# }
```

## Contract

- **Time** is `core::time::Duration` since an epoch the caller picks, for
  instants (`now`, history entries) and spans (the intensity window, the
  restart delay) alike. The core reads no clock.
- **One refusal.** `Refusal::UnknownChild`: `stopped` was asked about an index
  not below `CHILDREN`. It is checked first, on a terminated supervisor too,
  and a refusal changes nothing, not even the history.
- **Every `Decision` is honoured.** `Ignore`, `Restart(Span)`, `Escalate` and
  `AlreadyEscalated` are all answers to a well-formed question.
- **Honoured repeat.** Once escalated, the supervisor answers every further
  `stopped` with `AlreadyEscalated`: provable because `terminated()` is part of
  its state, and it records nothing.
- **The clock.** The supervisor has no observed clock of its own; its only
  memory of time is the history, a sorted set of restart instants. An answer
  that depends on `now` records it (`Restart`, `Escalate`, `Recorded`); answers
  that hold whatever the time (`Ignore`, `AlreadyEscalated`, `Outdated`)
  record nothing. Restarts may be reported in any order, since processes
  sharing one history seldom agree on one; there is no time-regression
  refusal.
- **No issued ids.** Child indices are the caller's own numbers, so there is
  no scope. `restore` rebuilds from a saved history; escalation is not part of
  it, and a restored supervisor decides for itself.
- **Shape.** `Supervisor::new` answers `None` for no children or for a
  `HISTORY` that cannot hold one more restart than the intensity tolerates
  (it could never escalate); `Intensity::new` answers `None` for a zero
  window; `Span::new` answers `None` when `first` is past `last`.

## Every answer

| Method | Answers | Records the clock |
| --- | --- | --- |
| `Supervisor::new(strategy, intensity)` | `Some(Supervisor)`, `None` for a shape that could decide nothing | - |
| `Supervisor::restore(strategy, intensity, history, now)` | `Some(Supervisor)` holding the restarts inside the window as of `now`, `None` as `new` | - |
| `stopped(index, policy, exit, now)` | `Ok(Ignore)` | no |
| | `Ok(Restart(span))`: one restart recorded at `now` | yes |
| | `Ok(Escalate)`: recorded at `now`, now terminated | yes |
| | `Ok(AlreadyEscalated)`: terminated before the call | no |
| | `Err(UnknownChild)`: nothing changed | no |
| `record_external_restart(now)` | `Recorded`: `now` is in the history | yes |
| | `Outdated`: a full history of newer restarts, nothing changed | no |
| `absorb(&history, now)` | how many restarts were added (larger count of each instant kept); `0` changes nothing | only the added instants |
| `recent_restarts(now)`, `restart_delay(base, ceiling, now)`, `span(index)` | pure queries (`span` is `None` past `CHILDREN`) | no |
| `history()`, `strategy()`, `intensity()`, `terminated()` | readers | no |

## Strategies

`OneForOne` restarts only what died. `OneForAll` restarts every child, for a set
that only works together. `RestForOne` restarts the child and everything started
after it, for a chain where later children depend on earlier ones.

One restart is recorded per failure handled, not per child brought back, so the
strategy does not change how fast the intensity is spent. That is OTP's
accounting.

## Restart delay

`Decision::Restart` says what to bring back, not when, which is also OTP. Act on
it immediately and a child that dies while being constructed spends the whole
intensity budget in well under a millisecond, so the window the intensity was
written for never means anything.

`restart_delay(base, ceiling, now)` is how long to wait first: `base` doubled
once per restart already inside the window, capped at `ceiling`. The count
decays as the window slides, so a child that settles returns to `base` on its
own. Jitter is the caller's to add.

```rust
# use core::time::Duration;
# use supervisor_machines::{Exit, Intensity, Policy, Strategy, Supervisor};
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let ms = Duration::from_millis;
# let intensity = Intensity::new(5, ms(60_000)).ok_or("zero window")?;
# let mut tree: Supervisor<1, 8> =
#     Supervisor::new(Strategy::OneForOne, intensity).ok_or("shape")?;
tree.stopped(0, Policy::Permanent, Exit::Abnormal, ms(0))?;
assert_eq!(tree.restart_delay(ms(100), ms(10_000), ms(0)), ms(100));

tree.stopped(0, Policy::Permanent, Exit::Abnormal, ms(1))?;
assert_eq!(tree.restart_delay(ms(100), ms(10_000), ms(1)), ms(200));
# Ok(())
# }
```

## Sharing a history

A daemon that several processes may each respawn needs one restart history, or
every process starts with a fresh budget and a crash-looping child is respawned
forever. `history()` yields the restart instants, oldest first; persist them
next to the daemon's lock. `restore` rebuilds a supervisor from them in another
process, `absorb` merges a history into a running one, and
`record_external_restart` counts a single restart someone else performed.
`restore` and `absorb` drop restarts the window has slid past. `restore` keeps
the saved history as a multiset: two restarts at one instant restore as two.
`absorb` takes the history as a slice, in any order, and keeps the larger
count of each instant across both: another process's two restarts in one
nanosecond stay two, and an absorb repeated changes nothing and answers `0`. `record_external_restart` counts every call: two restarts in
the same nanosecond are two restarts.

```rust
use core::time::Duration;
use supervisor_machines::{Decision, Exit, Intensity, Policy, Strategy, Supervisor};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let seconds = Duration::from_secs;
let intensity = Intensity::new(2, seconds(60)).ok_or("zero window")?;
let mut first: Supervisor<1, 8> =
    Supervisor::new(Strategy::OneForOne, intensity).ok_or("shape")?;
first.stopped(0, Policy::Permanent, Exit::Abnormal, seconds(10))?;
first.stopped(0, Policy::Permanent, Exit::Abnormal, seconds(20))?;

// Another process reads what the first wrote down.
let mut second: Supervisor<1, 8> =
    Supervisor::restore(Strategy::OneForOne, intensity, first.history(), seconds(25))
        .ok_or("shape")?;
assert_eq!(second.recent_restarts(seconds(25)), 2);
assert_eq!(
    second.stopped(0, Policy::Permanent, Exit::Abnormal, seconds(30)),
    Ok(Decision::Escalate),
);
# Ok(())
# }
```

Escalation is not shared: each process decides for itself from the merged
history.

## Policies

`Permanent` always comes back, even from a clean exit. `Transient` comes back
only from a bad one, because a clean exit means it finished. `Temporary` never
comes back, and never spends the restart budget.

```rust
use core::time::Duration;
use supervisor_machines::{Decision, Exit, Intensity, Policy, Strategy, Supervisor};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut tree: Supervisor<2, 4> =
    Supervisor::new(Strategy::OneForOne, Intensity::default()).ok_or("shape")?;

let now = Duration::ZERO;
assert_eq!(tree.stopped(0, Policy::Transient, Exit::Normal, now), Ok(Decision::Ignore));
assert_eq!(tree.stopped(0, Policy::Temporary, Exit::Abnormal, now), Ok(Decision::Ignore));
# Ok(())
# }
```

## Shape

`CHILDREN` fixes the strategy spans; an index at or past it is refused with
`UnknownChild`. `HISTORY` is how many restart instants are kept; it needs to
exceed the intensity's restart count, and `Supervisor::new` answers `None`
otherwise.

## License

MIT.
