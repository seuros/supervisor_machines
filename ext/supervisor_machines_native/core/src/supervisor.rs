//! The decision machine itself.

use core::time::Duration;

use crate::{Decision, Exit, Intensity, Policy, Recording, Refusal, Span, Strategy};

/// Holds the strategy, the restart intensity, and the recent restart history.
///
/// `CHILDREN` is how many children the supervisor owns, which fixes the
/// strategy spans. `HISTORY` is the restart instants kept; it must exceed the
/// intensity's restart count, or a burst past the intensity could not be
/// measured: [`new`](Self::new) answers `None` otherwise.
///
/// The supervisor keeps no clock of its own. Its only memory of time is the
/// history, a sorted multiset of restart instants, so restarts may be reported in
/// any order (several processes sharing one history seldom agree on one).
///
/// ```
/// use core::time::Duration;
/// use supervisor_machines::{Decision, Exit, Intensity, Policy, Span, Strategy, Supervisor};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Three children that only work as a set, two restarts per minute allowed.
/// let intensity = Intensity::new(2, Duration::from_secs(60)).ok_or("zero window")?;
/// let mut tree: Supervisor<3, 8> =
///     Supervisor::new(Strategy::OneForAll, intensity).ok_or("shape")?;
///
/// // The middle child dies badly, so all three come back.
/// let decision = tree.stopped(1, Policy::Permanent, Exit::Abnormal, Duration::ZERO)?;
/// assert_eq!(Some(decision), Span::new(0, 2).map(Decision::Restart));
///
/// // A temporary child is never worth restarting.
/// assert_eq!(
///     tree.stopped(2, Policy::Temporary, Exit::Abnormal, Duration::from_secs(1)),
///     Ok(Decision::Ignore),
/// );
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Supervisor<const CHILDREN: usize, const HISTORY: usize> {
    strategy: Strategy,
    intensity: Intensity,
    history: [Duration; HISTORY],
    recorded: usize,
    terminated: bool,
}

impl<const CHILDREN: usize, const HISTORY: usize> Supervisor<CHILDREN, HISTORY> {
    /// A supervisor with an empty history, or `None` for a shape that could
    /// decide nothing: no children, or a history too short to hold one more
    /// restart than the intensity tolerates.
    #[must_use]
    pub fn new(strategy: Strategy, intensity: Intensity) -> Option<Self> {
        // A restart count past `usize` is past any history.
        let tolerated = usize::try_from(intensity.restarts()).unwrap_or(usize::MAX);
        if CHILDREN == 0 || HISTORY <= tolerated {
            return None;
        }
        Some(Self {
            strategy,
            intensity,
            history: [Duration::ZERO; HISTORY],
            recorded: 0,
            terminated: false,
        })
    }

    /// A supervisor that already holds `history`, as another process left it.
    /// `None` exactly when [`new`](Self::new) would answer `None`.
    ///
    /// The saved history is a multiset, as [`history`](Self::history)
    /// reports it: every instant counts, so two restarts at one instant
    /// restore as two ([`absorb`](Self::absorb) instead keeps the larger
    /// count of each instant across both histories). Restarts older than the intensity
    /// window as of `now` are dropped on the way in, and when more remain
    /// than `HISTORY` can hold the oldest go.
    /// Escalation is not part of the saved state: a restored supervisor is
    /// not terminated, and decides for itself at its next
    /// [`stopped`](Self::stopped). Nothing here is an issued id, so there is
    /// no scope to restore under.
    ///
    /// ```
    /// use core::time::Duration;
    /// use supervisor_machines::{Intensity, Strategy, Supervisor};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let seconds = Duration::from_secs;
    /// let intensity = Intensity::new(3, seconds(60)).ok_or("zero window")?;
    /// let mut before: Supervisor<1, 8> =
    ///     Supervisor::new(Strategy::OneForOne, intensity).ok_or("shape")?;
    /// before.record_external_restart(seconds(10));
    /// before.record_external_restart(seconds(20));
    ///
    /// // Persist `before.history()`, then rebuild it elsewhere.
    /// let after: Supervisor<1, 8> = Supervisor::restore(
    ///     before.strategy(),
    ///     before.intensity(),
    ///     before.history(),
    ///     seconds(30),
    /// )
    /// .ok_or("shape")?;
    /// assert_eq!(after.recent_restarts(seconds(30)), 2);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn restore(
        strategy: Strategy,
        intensity: Intensity,
        history: impl IntoIterator<Item = Duration>,
        now: Duration,
    ) -> Option<Self> {
        let mut supervisor = Self::new(strategy, intensity)?;
        let cutoff = now.saturating_sub(intensity.window());
        for at in history.into_iter().filter(|&at| at >= cutoff) {
            // `false` is an instant older than a full history of newer
            // ones: dropping it is what keeping the newest means.
            supervisor.record(at);
        }
        Some(supervisor)
    }

    /// The strategy it was built with.
    #[must_use]
    pub const fn strategy(&self) -> Strategy {
        self.strategy
    }

    /// The intensity it was built with.
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

    /// Every restart instant the supervisor still holds, oldest first.
    ///
    /// An iterator over the supervisor's own storage, not an output buffer:
    /// it borrows, copies nothing and allocates nothing. This is what to
    /// persist so another process can [`restore`](Self::restore) or
    /// [`absorb`](Self::absorb) it.
    pub fn history(&self) -> impl Iterator<Item = Duration> + '_ {
        self.history[..self.recorded].iter().copied()
    }

    /// Restarts still inside the window as of `now`. A pure query.
    #[must_use]
    pub fn recent_restarts(&self, now: Duration) -> u32 {
        let cutoff = now.saturating_sub(self.intensity.window());
        let recent = self.history[..self.recorded]
            .iter()
            .rev()
            .take_while(|&&at| at >= cutoff)
            .count();
        u32::try_from(recent).unwrap_or(u32::MAX)
    }

    /// Count a restart some other process performed at `now`.
    ///
    /// It spends the intensity budget exactly as a restart this supervisor
    /// granted would, but decides nothing: escalation waits for the next
    /// [`stopped`](Self::stopped). Never refused: any instant is a restart
    /// that happened. Two calls with the same instant are two restarts.
    pub fn record_external_restart(&mut self, now: Duration) -> Recording {
        if self.record(now) {
            Recording::Recorded
        } else {
            Recording::Outdated
        }
    }

    /// Merge restarts another process holds, as its [`history`](Self::history)
    /// reported them (in any order), and answer how many were added.
    ///
    /// Both histories are multisets and the merge keeps the larger count of
    /// each instant: an instant held twice here and once in `history` stays
    /// twice, one held once here and twice there ends up twice. So absorbing
    /// the same history twice changes nothing and answers zero, the order of
    /// two absorbs does not matter, and another process's two restarts in one
    /// nanosecond stay two. Restarts in `history` older than the window as of
    /// `now` are dropped. When the merge holds more than `HISTORY` the oldest
    /// go, as for [`record_external_restart`](Self::record_external_restart),
    /// and the answer counts only the instants still held. Never refused.
    ///
    /// Allocates nothing. A sorted `history` (as [`history`](Self::history)
    /// yields it) is merged in one pass; an unsorted one costs a scan of the
    /// earlier entries per entry.
    pub fn absorb(&mut self, history: &[Duration], now: Duration) -> usize {
        let cutoff = now.saturating_sub(self.intensity.window());
        // The counts to compare against are this supervisor's before the
        // merge: inserting shifts (and, when full, drops) what it holds.
        let snapshot = self.history;
        let before = &snapshot[..self.recorded];
        let sorted = history.is_sorted();
        let mut previous = None;
        let mut run = 0;
        for (index, &at) in history.iter().enumerate() {
            if sorted {
                run = if previous == Some(at) { run + 1 } else { 0 };
                previous = Some(at);
            }
            if at < cutoff {
                continue;
            }
            // How many times `at` came earlier in `history`: this entry is
            // its occurrence number `rank`, wanted only past what is held.
            let rank = if sorted {
                run
            } else {
                history[..index]
                    .iter()
                    .filter(|&&earlier| earlier == at)
                    .count()
            };
            if rank >= occurrences(before, at) {
                // `false` is an instant older than a full history of newer
                // ones: dropping it is what keeping the newest means.
                self.record(at);
            }
        }
        added(before, &self.history[..self.recorded])
    }

    /// How long to hold off the restart just granted: `base` doubled once per
    /// restart already inside the window, capped at `ceiling`. A pure query.
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
    /// to `base` on its own. Jitter is the caller's to add.
    ///
    /// ```
    /// use core::time::Duration;
    /// use supervisor_machines::{Exit, Intensity, Policy, Strategy, Supervisor};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ms = Duration::from_millis;
    /// let intensity = Intensity::new(5, ms(60_000)).ok_or("zero window")?;
    /// let mut tree: Supervisor<1, 8> =
    ///     Supervisor::new(Strategy::OneForOne, intensity).ok_or("shape")?;
    ///
    /// // The first restart waits the base, each further one twice as long.
    /// tree.stopped(0, Policy::Permanent, Exit::Abnormal, ms(0))?;
    /// assert_eq!(tree.restart_delay(ms(100), ms(10_000), ms(0)), ms(100));
    ///
    /// tree.stopped(0, Policy::Permanent, Exit::Abnormal, ms(1))?;
    /// assert_eq!(tree.restart_delay(ms(100), ms(10_000), ms(1)), ms(200));
    ///
    /// // Once the window has slid past them the delay is back to the base.
    /// assert_eq!(tree.restart_delay(ms(100), ms(10_000), ms(120_000)), ms(100));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn restart_delay(&self, base: Duration, ceiling: Duration, now: Duration) -> Duration {
        let mut delay = base;
        let mut doublings = self.recent_restarts(now).saturating_sub(1);
        while doublings > 0 && delay < ceiling {
            delay = delay.saturating_mul(2);
            doublings -= 1;
        }
        delay.min(ceiling)
    }

    /// The children a failure at `index` takes with it under this strategy,
    /// or `None` when the supervisor owns no child at `index`. A pure query.
    #[must_use]
    pub const fn span(&self, index: usize) -> Option<Span> {
        if index >= CHILDREN {
            return None;
        }
        let last = CHILDREN - 1;
        match self.strategy {
            Strategy::OneForOne => Span::new(index, index),
            Strategy::OneForAll => Span::new(0, last),
            Strategy::RestForOne => Span::new(index, last),
        }
    }

    /// Insert `at` in order. A full history lets its oldest instant go; an
    /// instant older than everything a full history holds is not kept, and
    /// the answer is `false` with nothing changed.
    fn record(&mut self, at: Duration) -> bool {
        let slot = self.history[..self.recorded].partition_point(|&held| held <= at);
        if self.recorded < HISTORY {
            self.history.copy_within(slot..self.recorded, slot + 1);
            self.history[slot] = at;
            self.recorded += 1;
            return true;
        }
        if slot == 0 {
            return false;
        }
        self.history.copy_within(1..slot, 0);
        self.history[slot - 1] = at;
        true
    }

    /// Decide what to do about the child at `index`.
    ///
    /// One restart is recorded per failure handled, not per child restarted,
    /// and only by the answers that depend on `now`:
    /// [`Decision::Restart`] and [`Decision::Escalate`].
    /// [`Decision::Ignore`] and [`Decision::AlreadyEscalated`] hold whatever the
    /// time and record nothing.
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownChild`] when `index` is not below `CHILDREN`, on a
    /// terminated supervisor too. Nothing changed.
    pub fn stopped(
        &mut self,
        index: usize,
        policy: Policy,
        exit: Exit,
        now: Duration,
    ) -> Result<Decision, Refusal> {
        let Some(span) = self.span(index) else {
            return Err(Refusal::UnknownChild);
        };
        if self.terminated {
            return Ok(Decision::AlreadyEscalated);
        }
        match (policy, exit) {
            (Policy::Temporary, Exit::Normal | Exit::Abnormal)
            | (Policy::Transient, Exit::Normal) => return Ok(Decision::Ignore),
            (Policy::Permanent, Exit::Normal | Exit::Abnormal)
            | (Policy::Transient, Exit::Abnormal) => {}
        }
        if !self.record(now) {
            // Only a full history of newer restarts leaves `now` out, and all
            // of those are inside the window, past what the intensity
            // tolerates: the answer below escalates either way.
            debug_assert!(
                self.recent_restarts(now) > self.intensity.restarts(),
                "a restart left out of the history is one the intensity already exceeds"
            );
        }
        if self.recent_restarts(now) > self.intensity.restarts() {
            self.terminated = true;
            return Ok(Decision::Escalate);
        }
        Ok(Decision::Restart(span))
    }
}

/// How many times `at` appears in the sorted `held`.
fn occurrences(held: &[Duration], at: Duration) -> usize {
    let after = held.partition_point(|&instant| instant <= at);
    after - held[..after].partition_point(|&instant| instant < at)
}

/// How many entries of the sorted `after` are not matched by one in the
/// sorted `before`: the multiset difference, so a full history that swapped
/// an instant for an equal one added nothing.
fn added(before: &[Duration], after: &[Duration]) -> usize {
    let mut held = before.iter().peekable();
    let mut added = 0;
    for at in after {
        while held.next_if(|&instant| instant < at).is_some() {}
        if held.next_if_eq(&at).is_none() {
            added += 1;
        }
    }
    added
}

#[cfg(test)]
mod tests;
