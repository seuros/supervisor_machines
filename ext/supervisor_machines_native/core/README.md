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
