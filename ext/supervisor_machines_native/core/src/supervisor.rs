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
            terminated: false,
        }
    }

    /// A supervisor that already holds `history`, as another process left it.
    ///
    /// Restarts older than the intensity window as of `now_ns` are dropped on
    /// the way in, and when more remain than `HISTORY` can hold the oldest go.
    ///
    /// ```
    /// use supervisor_machines::{Intensity, Strategy, Supervisor};
    ///
    /// const SECOND: u64 = 1_000_000_000;
    ///
    /// let mut before: Supervisor<1, 8> =
    ///     Supervisor::new(Strategy::OneForOne, Intensity::new(3, 60 * SECOND));
    /// before.record_external_restart(10 * SECOND);
    /// before.record_external_restart(20 * SECOND);
    ///
    /// // Persist `before.history()`, then rebuild it elsewhere.
    /// let after: Supervisor<1, 8> = Supervisor::restore(
    ///     before.strategy(),
    ///     before.intensity(),
    ///     before.history(),
    ///     30 * SECOND,
    /// );
    /// assert_eq!(after.recent_restarts(30 * SECOND), 2);
    /// ```
    #[must_use]
    pub fn restore(
        strategy: Strategy,
        intensity: Intensity,
        history: impl IntoIterator<Item = u64>,
        now_ns: u64,
    ) -> Self {
        let mut supervisor = Self::new(strategy, intensity);
        supervisor.absorb(history, now_ns);
        supervisor
    }

    #[must_use]
    pub const fn strategy(&self) -> Strategy {
        self.strategy
    }

    #[must_use]
    pub const fn intensity(&self) -> Intensity {
        self.intensity
    }

    /// True once the supervisor has escalated. It stays true: a supervisor that
    /// gave up does not change its mind.
    #[must_use]
    pub const fn terminated(&self) -> bool {
        self.terminated
    }

    /// Every restart the supervisor still holds, oldest first, in nanoseconds.
    ///
    /// This is what to persist so another process can
    /// [`restore`](Self::restore) or [`absorb`](Self::absorb) it.
    pub fn history(&self) -> impl Iterator<Item = u64> + '_ {
        self.history[..self.recorded].iter().copied()
    }

    /// Restarts still inside the window as of `now_ns`.
    #[must_use]
    pub const fn recent_restarts(&self, now_ns: u64) -> u32 {
        let cutoff = now_ns.saturating_sub(self.intensity.window_ns);
        let mut counted = 0;
        let mut index = self.recorded;
        while index > 0 && self.history[index - 1] >= cutoff {
            counted += 1;
            index -= 1;
        }
        counted
    }

    /// Count a restart some other process performed at `now_ns`.
    ///
    /// It spends the intensity budget exactly as a restart this supervisor
    /// granted would, but decides nothing: escalation waits for the next
    /// [`stopped`](Self::stopped).
    pub const fn record_external_restart(&mut self, now_ns: u64) {
        self.record(now_ns);
    }

    /// Merge restarts another process holds, as its [`history`](Self::history)
    /// reported them.
    ///
    /// Restarts older than the window as of `now_ns` are dropped, and a
    /// timestamp this supervisor already holds is not counted again, so
    /// absorbing the same history twice changes nothing.
    pub fn absorb(&mut self, history: impl IntoIterator<Item = u64>, now_ns: u64) {
        let cutoff = now_ns.saturating_sub(self.intensity.window_ns);
        for at_ns in history {
            if at_ns >= cutoff && !self.holds(at_ns) {
                self.record(at_ns);
            }
        }
    }

    const fn holds(&self, at_ns: u64) -> bool {
        let mut index = 0;
        while index < self.recorded {
            if self.history[index] == at_ns {
                return true;
            }
            index += 1;
        }
        false
    }

    /// How long to hold off the restart just granted: `base_ns` doubled once
    /// per restart already inside the window, capped at `ceiling_ns`.
    ///
    /// Neither OTP nor [`stopped`](Self::stopped) imposes a delay, so a
    /// supervisor that restarts the moment a child dies spends its whole
    /// intensity budget in the time it takes to fail that many times. For a
    /// child that dies while being constructed that is well under a
    /// millisecond, and the window the intensity was written for never gets to
    /// mean anything. Waiting this long between the decision and the restart
    /// spreads the budget back over that window.
    ///
    /// The count decays as the window slides, so a child that settles returns
    /// to `base_ns` on its own. Jitter is the caller's to add.
    ///
    /// ```
    /// use supervisor_machines::{Exit, Intensity, Policy, Strategy, Supervisor};
    ///
    /// const MS: u64 = 1_000_000;
    ///
    /// let mut tree: Supervisor<1, 8> = Supervisor::new(
    ///     Strategy::OneForOne,
    ///     Intensity::new(5, 60_000 * MS),
    /// );
    ///
    /// // The first restart waits the base, each further one twice as long.
    /// tree.stopped(0, Policy::Permanent, Exit::Abnormal, 0);
    /// assert_eq!(tree.restart_delay_ns(100 * MS, 10_000 * MS, 0), 100 * MS);
    ///
    /// tree.stopped(0, Policy::Permanent, Exit::Abnormal, MS);
    /// assert_eq!(tree.restart_delay_ns(100 * MS, 10_000 * MS, MS), 200 * MS);
    ///
    /// // Once the window has slid past them the delay is back to the base.
    /// assert_eq!(
    ///     tree.restart_delay_ns(100 * MS, 10_000 * MS, 120_000 * MS),
    ///     100 * MS,
    /// );
    /// ```
    #[must_use]
    pub const fn restart_delay_ns(&self, base_ns: u64, ceiling_ns: u64, now_ns: u64) -> u64 {
        let mut delay = base_ns;
        let mut doublings = self.recent_restarts(now_ns).saturating_sub(1);
        while doublings > 0 && delay < ceiling_ns {
            delay = delay.saturating_mul(2);
            doublings -= 1;
        }
        if delay > ceiling_ns {
            ceiling_ns
        } else {
            delay
        }
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

    const fn record(&mut self, at_ns: u64) {
        let mut slot = self.recorded;
        while slot > 0 && self.history[slot - 1] > at_ns {
            slot -= 1;
        }
        if self.recorded < HISTORY {
            let mut index = self.recorded;
            while index > slot {
                self.history[index] = self.history[index - 1];
                index -= 1;
            }
            self.history[slot] = at_ns;
            self.recorded += 1;
        } else if slot > 0 {
            let mut index = 1;
            while index < slot {
                self.history[index - 1] = self.history[index];
                index += 1;
            }
            self.history[slot - 1] = at_ns;
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
