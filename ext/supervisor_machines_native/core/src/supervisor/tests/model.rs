//! A randomized run against a reference model that keeps the history in a
//! `Vec` (a multiset: repeated instants count) and recomputes every answer
//! from scratch.

extern crate std;

use core::time::Duration;
use std::vec::Vec;

use matryona_machines::testing::{XorShift64, refuses, unchanged};

use super::{intensity, some};
use crate::{Decision, Exit, Policy, Recording, Refusal, Span, Strategy, Supervisor};

const CHILDREN: usize = 5;
const HISTORY: usize = 6;

struct Model {
    strategy: Strategy,
    restarts: u32,
    window: Duration,
    history: Vec<Duration>,
    terminated: bool,
}

impl Model {
    fn recent(&self, now: Duration) -> u32 {
        let cutoff = now.saturating_sub(self.window);
        // The newest restarts at or after the cutoff, counted from the end.
        let recent = self.history.iter().rev().take_while(|&&at| at >= cutoff);
        u32::try_from(recent.count()).unwrap_or(u32::MAX)
    }

    fn record(&mut self, at: Duration) -> bool {
        if self.history.len() == HISTORY && self.history.iter().all(|&held| held > at) {
            return false;
        }
        self.history.push(at);
        self.history.sort();
        if self.history.len() > HISTORY {
            self.history.remove(0);
        }
        true
    }

    /// Max-multiplicity merge: each instant inside the window ends up held
    /// as often as the side holding it more often holds it, then the newest
    /// `HISTORY` stay. Answers the multiset difference from before.
    fn absorb(&mut self, theirs: &[Duration], now: Duration) -> usize {
        let cutoff = now.saturating_sub(self.window);
        let before = self.history.clone();
        let count = |list: &[Duration], at: Duration| list.iter().filter(|&&x| x == at).count();
        let mut distinct: Vec<Duration> =
            theirs.iter().copied().filter(|&at| at >= cutoff).collect();
        distinct.sort();
        distinct.dedup();
        for at in distinct {
            for _ in count(&before, at)..count(theirs, at) {
                self.record(at);
            }
        }
        let mut left = before;
        self.history
            .iter()
            .filter(|&&at| {
                left.iter()
                    .position(|&held| held == at)
                    .map(|slot| left.remove(slot))
                    .is_none()
            })
            .count()
    }

    fn span(&self, index: usize) -> Option<Span> {
        if index >= CHILDREN {
            return None;
        }
        match self.strategy {
            Strategy::OneForOne => Span::new(index, index),
            Strategy::OneForAll => Span::new(0, CHILDREN - 1),
            Strategy::RestForOne => Span::new(index, CHILDREN - 1),
        }
    }

    fn stopped(
        &mut self,
        index: usize,
        policy: Policy,
        exit: Exit,
        now: Duration,
    ) -> Result<Decision, Refusal> {
        let span = self.span(index).ok_or(Refusal::UnknownChild)?;
        if self.terminated {
            return Ok(Decision::AlreadyEscalated);
        }
        let restartable = match policy {
            Policy::Permanent => true,
            Policy::Transient => exit == Exit::Abnormal,
            Policy::Temporary => false,
        };
        if !restartable {
            return Ok(Decision::Ignore);
        }
        self.record(now);
        if self.recent(now) > self.restarts {
            self.terminated = true;
            return Ok(Decision::Escalate);
        }
        Ok(Decision::Restart(span))
    }
}

/// The supervisor under test.
type Real = Supervisor<CHILDREN, HISTORY>;

fn run(seed: u64, strategy: Strategy, restarts: u32) {
    let window = Duration::from_nanos(40);
    let mut random = XorShift64::new(seed);
    let mut real: Real = some(Supervisor::new(strategy, intensity(restarts, window)));
    let mut model = Model {
        strategy,
        restarts,
        window,
        history: Vec::new(),
        terminated: false,
    };
    let mut clock = 0;

    for _ in 0..600 {
        clock += random.below(12);
        // Mostly the present, sometimes another process's stale or early clock.
        let now = Duration::from_nanos((clock + random.below(9)).saturating_sub(random.below(30)));
        let before = real;
        match random.below(10) {
            0..=5 => {
                let index = usize::try_from(random.below(7)).unwrap_or(0);
                let policy = [Policy::Permanent, Policy::Transient, Policy::Temporary]
                    [usize::try_from(random.below(3)).unwrap_or(0)];
                let exit =
                    [Exit::Normal, Exit::Abnormal][usize::try_from(random.below(2)).unwrap_or(0)];
                let expected = model.stopped(index, policy, exit, now);
                let stop = |r: &mut Real| r.stopped(index, policy, exit, now);
                match expected {
                    Err(refusal @ Refusal::UnknownChild) => refuses(&mut real, refusal, stop),
                    Ok(Decision::Ignore | Decision::AlreadyEscalated) => {
                        assert_eq!(unchanged(&mut real, stop), expected);
                    }
                    Ok(Decision::Restart(span)) => {
                        assert_eq!(stop(&mut real), expected);
                        assert!(span.contains(index) && span.last() < CHILDREN);
                        assert_ne!(real, before);
                    }
                    Ok(Decision::Escalate) => {
                        assert_eq!(stop(&mut real), expected);
                        assert!(real.terminated() && !before.terminated());
                    }
                }
            }
            6 | 7 => {
                let record = |r: &mut Real| r.record_external_restart(now);
                if model.record(now) {
                    assert_eq!(record(&mut real), Recording::Recorded);
                } else {
                    assert_eq!(unchanged(&mut real, record), Recording::Outdated);
                }
            }
            8 => {
                let mut theirs: Vec<Duration> = (0..random.below(6))
                    .map(|_| Duration::from_nanos(clock.saturating_sub(random.below(30))))
                    .collect();
                if random.below(2) == 0 {
                    // As another supervisor's `history()` would list them.
                    theirs.sort();
                }
                let expected = model.absorb(&theirs, now);
                if expected == 0 {
                    assert_eq!(unchanged(&mut real, |r| r.absorb(&theirs, now)), 0);
                } else {
                    assert_eq!(real.absorb(&theirs, now), expected);
                }
                // A repeat absorbs nothing, and its own history adds nothing.
                assert_eq!(unchanged(&mut real, |r| r.absorb(&theirs, now)), 0);
                let own: Vec<Duration> = real.history().collect();
                assert_eq!(unchanged(&mut real, |r| r.absorb(&own, now)), 0);
            }
            _ => {
                let base = Duration::from_nanos(1 + random.below(5));
                let ceiling = Duration::from_nanos(random.below(40));
                let mut delay = base;
                for _ in 1..model.recent(now) {
                    delay = delay.saturating_mul(2);
                }
                assert_eq!(real.restart_delay(base, ceiling, now), delay.min(ceiling));
                assert_eq!(real, before);
            }
        }
        assert!(real.history().eq(model.history.iter().copied()));
        assert!(real.history().is_sorted());
        assert_eq!(real.terminated(), model.terminated);
        assert_eq!(real.recent_restarts(now), model.recent(now));
        for index in 0..CHILDREN + 2 {
            assert_eq!(real.span(index), model.span(index));
        }
    }
}

#[test]
fn the_supervisor_agrees_with_the_reference_model() {
    let strategies = [
        Strategy::OneForOne,
        Strategy::OneForAll,
        Strategy::RestForOne,
    ];
    for seed in 1..=60 {
        for restarts in [0, 2, 5] {
            run(
                seed * 0x9E37_79B9,
                strategies[(seed % 3) as usize],
                restarts,
            );
        }
    }
}
