# supervisor-machines

Supervision decisions, in the OTP sense. `no_std`, zero dependencies.

A child died. Which children restart, and has that happened too often to keep
trying? That is all this crate answers. It never spawns, kills or waits for
anything, so it works the same whether the caller supervises threads, fibers or
processes.

```rust
use supervisor_machines::{Decision, Exit, Intensity, Policy, Span, Strategy, Supervisor};

const SECOND: u64 = 1_000_000_000;

// Three children that only work as a set, two restarts per minute allowed.
let mut tree: Supervisor<3, 8> = Supervisor::new(
    Strategy::OneForAll,
    Intensity::new(2, 60 * SECOND),
);

// The middle child dies badly, so all three come back.
assert_eq!(
    tree.stopped(1, Policy::Permanent, Exit::Abnormal, 0),
    Decision::Restart(Span { first: 0, last: 2 }),
);

// Keep failing and the supervisor gives up rather than thrashing.
tree.stopped(1, Policy::Permanent, Exit::Abnormal, SECOND);
assert_eq!(
    tree.stopped(1, Policy::Permanent, Exit::Abnormal, 2 * SECOND),
    Decision::Escalate,
);
assert!(tree.terminated());
```

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

`restart_delay_ns(base, ceiling, now)` is how long to wait first: `base` doubled
once per restart already inside the window, capped at `ceiling`. The count
decays as the window slides, so a child that settles returns to `base` on its
own. Jitter is the caller's to add.

```rust
# use supervisor_machines::{Exit, Intensity, Policy, Strategy, Supervisor};
# const MS: u64 = 1_000_000;
# let mut tree: Supervisor<1, 8> = Supervisor::new(
#     Strategy::OneForOne,
#     Intensity::new(5, 60_000 * MS),
# );
tree.stopped(0, Policy::Permanent, Exit::Abnormal, 0);
assert_eq!(tree.restart_delay_ns(100 * MS, 10_000 * MS, 0), 100 * MS);

tree.stopped(0, Policy::Permanent, Exit::Abnormal, MS);
assert_eq!(tree.restart_delay_ns(100 * MS, 10_000 * MS, MS), 200 * MS);
```

## Sharing a history

A daemon that several processes may each respawn needs one restart history, or
every process starts with a fresh budget and a crash-looping child is respawned
forever. `history()` yields the restart timestamps, oldest first; persist them
next to the daemon's lock. `restore` rebuilds a supervisor from them in another
process, `absorb` merges a history into a running one, and
`record_external_restart` counts a single restart someone else performed. All
three drop restarts the window has slid past, and absorbing a timestamp already
held counts nothing twice.

```rust
use supervisor_machines::{Decision, Exit, Intensity, Policy, Strategy, Supervisor};

const SECOND: u64 = 1_000_000_000;

let intensity = Intensity::new(2, 60 * SECOND);
let mut first: Supervisor<1, 8> = Supervisor::new(Strategy::OneForOne, intensity);
first.stopped(0, Policy::Permanent, Exit::Abnormal, 10 * SECOND);
first.stopped(0, Policy::Permanent, Exit::Abnormal, 20 * SECOND);

// Another process reads what the first wrote down.
let mut second: Supervisor<1, 8> =
    Supervisor::restore(Strategy::OneForOne, intensity, first.history(), 25 * SECOND);
assert_eq!(second.recent_restarts(25 * SECOND), 2);
assert_eq!(
    second.stopped(0, Policy::Permanent, Exit::Abnormal, 30 * SECOND),
    Decision::Escalate,
);
```

Escalation is not shared: each process decides for itself from the merged
history.

## Policies

`Permanent` always comes back, even from a clean exit. `Transient` comes back
only from a bad one, because a clean exit means it finished. `Temporary` never
comes back, and never spends the restart budget.

```rust
use supervisor_machines::{Decision, Exit, Intensity, Policy, Strategy, Supervisor};

let mut tree: Supervisor<2, 4> = Supervisor::new(Strategy::OneForOne, Intensity::default());

assert_eq!(tree.stopped(0, Policy::Transient, Exit::Normal, 0), Decision::Ignore);
assert_eq!(tree.stopped(0, Policy::Temporary, Exit::Abnormal, 0), Decision::Ignore);
```

## Shape

`CHILDREN` fixes the strategy spans, so an index is checked by the type rather
than at runtime. `HISTORY` is how many restart timestamps are kept; it needs to
exceed the intensity's restart count, and both are asserted at compile time.

## License

MIT.
