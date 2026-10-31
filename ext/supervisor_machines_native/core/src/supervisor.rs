//! The decision machine itself.

use crate::{Decision, Exit, Intensity, Policy, Span, Strategy};

/// Holds the strategy, the restart intensity, and the recent restart history.
///
/// `CHILDREN` is how many children the supervisor owns, which fixes the
/// strategy spans. `HISTORY` is the restart timestamps kept; it must exceed the
/// intensity's restart count, or a single burst could not be measured.
///
/// ```
/// use supervisor_machines::{Decision, Exit, Intensity, Policy, Span, Strategy, Supervisor};
///
/// const SECOND: u64 = 1_000_000_000;
///
/// // Three children that only work as a set, two restarts per minute allowed.
/// let mut tree: Supervisor<3, 8> = Supervisor::new(
///     Strategy::OneForAll,
///     Intensity::new(2, 60 * SECOND),
/// );
///
/// // The middle child dies badly, so all three come back.
/// let decision = tree.stopped(1, Policy::Permanent, Exit::Abnormal, 0);
/// assert_eq!(decision, Decision::Restart(Span { first: 0, last: 2 }));
///
/// // A temporary child is never worth restarting.
/// assert_eq!(
///     tree.stopped(2, Policy::Temporary, Exit::Abnormal, SECOND),
///     Decision::Ignore,
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Supervisor<const CHILDREN: usize, const HISTORY: usize> {
    strategy: Strategy,
    intensity: Intensity,
    history: [u64; HISTORY],
    recorded: usize,
    next: usize,
    terminated: bool,
}

impl<const CHILDREN: usize, const HISTORY: usize> Supervisor<CHILDREN, HISTORY> {
    /// Rejects a shape that could not measure its own intensity.
    const SHAPE: () = {
        assert!(
            CHILDREN > 0,
            "a supervisor with no children has nothing to decide"
        );
        assert!(
            HISTORY > 0,
            "restart history needs room for at least one restart"
        );
    };

    #[must_use]
    pub const fn new(strategy: Strategy, intensity: Intensity) -> Self {
        let () = Self::SHAPE;
        Self {
            strategy,
            intensity,
            history: [0; HISTORY],
            recorded: 0,
            next: 0,
            terminated: false,
        }
    }

    #[must_use]
    pub const fn strategy(&self) -> Strategy {
        self.strategy
    }

    /// True once the supervisor has escalated. It stays true: a supervisor that
    /// gave up does not change its mind.
    #[must_use]
    pub const fn terminated(&self) -> bool {
        self.terminated
    }

    /// Restarts still inside the window as of `now_ns`.
    #[must_use]
    pub const fn recent_restarts(&self, now_ns: u64) -> u32 {
        let cutoff = now_ns.saturating_sub(self.intensity.window_ns);
        let mut counted = 0;
        let mut index = 0;
        while index < self.recorded {
            if self.history[index] >= cutoff {
                counted += 1;
            }
            index += 1;
        }
        counted
    }

    /// The children a failure at `index` takes with it under this strategy.
    #[must_use]
    pub const fn span(&self, index: usize) -> Span {
        match self.strategy {
            Strategy::OneForOne => Span::single(index),
            Strategy::OneForAll => Span {
                first: 0,
                last: CHILDREN - 1,
            },
            Strategy::RestForOne => Span {
                first: index,
                last: CHILDREN - 1,
            },
        }
    }

    const fn record(&mut self, now_ns: u64) {
        self.history[self.next] = now_ns;
        self.next = (self.next + 1) % HISTORY;
        if self.recorded < HISTORY {
            self.recorded += 1;
        }
    }

    /// Decide what to do about the child at `index`.
    ///
    /// One restart is recorded per failure handled, not per child restarted.
    pub const fn stopped(
        &mut self,
        index: usize,
        policy: Policy,
        exit: Exit,
        now_ns: u64,
    ) -> Decision {
        if self.terminated {
            return Decision::Escalate;
        }

        match policy {
            Policy::Temporary => return Decision::Ignore,
            Policy::Transient if matches!(exit, Exit::Normal) => return Decision::Ignore,
            Policy::Permanent | Policy::Transient => {}
        }

        self.record(now_ns);
        if self.recent_restarts(now_ns) > self.intensity.restarts {
            self.terminated = true;
            return Decision::Escalate;
        }

        Decision::Restart(self.span(index))
    }
}

#[cfg(test)]
mod tests;
