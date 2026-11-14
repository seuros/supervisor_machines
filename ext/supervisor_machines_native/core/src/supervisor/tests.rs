use core::time::Duration;

use crate::tests::some;
use crate::{Decision, Exit, Intensity, Policy, Span, Strategy, Supervisor};

mod history;
mod model;
mod refusal;

const SECOND: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);

fn seconds(count: u64) -> Duration {
    Duration::from_secs(count)
}

fn intensity(restarts: u32, window: Duration) -> Intensity {
    some(Intensity::new(restarts, window))
}

fn tree<const CHILDREN: usize>(strategy: Strategy) -> Supervisor<CHILDREN, 128> {
    some(Supervisor::new(strategy, intensity(100, MINUTE)))
}

fn small<const HISTORY: usize>(restarts: u32, window: Duration) -> Supervisor<3, HISTORY> {
    some(Supervisor::new(
        Strategy::OneForOne,
        intensity(restarts, window),
    ))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "compared against what stopped answers"
)]
fn restart(first: usize, last: usize) -> Result<Decision, crate::Refusal> {
    Ok(Decision::Restart(some(Span::new(first, last))))
}

const BAD: (Policy, Exit) = (Policy::Permanent, Exit::Abnormal);

/// Each strategy's span for a child that died, and the span query agrees.
#[test]
fn the_strategy_decides_which_children_restart() {
    let cases = [
        (Strategy::OneForOne, [(2, 2), (4, 4)]),
        (Strategy::OneForAll, [(0, 4), (0, 4)]),
        (Strategy::RestForOne, [(2, 4), (4, 4)]),
    ];
    for (strategy, spans) in cases {
        let mut supervisor = tree::<5>(strategy);
        for ((index, (first, last)), now) in [2, 4].into_iter().zip(spans).zip([0, 1]) {
            assert_eq!(supervisor.span(index), Span::new(first, last));
            assert_eq!(
                supervisor.stopped(index, BAD.0, BAD.1, seconds(now)),
                restart(first, last),
                "{strategy:?} child {index}"
            );
        }
        assert_eq!(supervisor.span(5), None);
    }
}

/// Temporary never comes back, transient only from a bad exit, permanent
/// even from a clean one.
#[test]
fn the_policy_decides_whether_a_child_restarts() {
    let cases = [
        (Policy::Temporary, Exit::Normal, false),
        (Policy::Temporary, Exit::Abnormal, false),
        (Policy::Transient, Exit::Normal, false),
        (Policy::Transient, Exit::Abnormal, true),
        (Policy::Permanent, Exit::Normal, true),
        (Policy::Permanent, Exit::Abnormal, true),
    ];
    for (policy, exit, restarts) in cases {
        let mut supervisor = tree::<3>(Strategy::OneForOne);
        let expected = if restarts {
            restart(1, 1)
        } else {
            Ok(Decision::Ignore)
        };
        assert_eq!(
            supervisor.stopped(1, policy, exit, Duration::ZERO),
            expected,
            "{policy:?} after {exit:?}"
        );
    }
}

/// An ignored stop holds whatever the time, so it records nothing at all.
#[test]
fn an_ignored_stop_changes_nothing() {
    let mut supervisor = small::<8>(1, MINUTE);
    let before = supervisor;

    for tick in 0..50 {
        assert_eq!(
            supervisor.stopped(0, Policy::Temporary, Exit::Abnormal, seconds(tick)),
            Ok(Decision::Ignore)
        );
    }

    assert_eq!(supervisor, before);
    assert_eq!(supervisor.recent_restarts(seconds(50)), 0);
}

#[test]
fn exceeding_the_intensity_escalates_once_and_stays_terminated() {
    let mut supervisor = small::<8>(2, MINUTE);

    assert_eq!(
        supervisor.stopped(0, BAD.0, BAD.1, Duration::ZERO),
        restart(0, 0)
    );
    assert_eq!(supervisor.stopped(0, BAD.0, BAD.1, SECOND), restart(0, 0));
    assert_eq!(
        supervisor.stopped(0, BAD.0, BAD.1, seconds(2)),
        Ok(Decision::Escalate)
    );
    assert!(supervisor.terminated());
    assert!(
        supervisor
            .history()
            .eq([Duration::ZERO, SECOND, seconds(2)])
    );

    // The repeat is honoured, says so, and records nothing: not even the clock.
    let before = supervisor;
    for (policy, exit) in [BAD, (Policy::Temporary, Exit::Normal)] {
        assert_eq!(
            supervisor.stopped(1, policy, exit, seconds(3)),
            Ok(Decision::AlreadyEscalated)
        );
        assert_eq!(supervisor, before);
    }
}

#[test]
fn restarts_outside_the_window_stop_counting() {
    let mut supervisor = small::<8>(1, seconds(5));
    assert!(supervisor.stopped(0, BAD.0, BAD.1, Duration::ZERO).is_ok());

    assert_eq!(supervisor.recent_restarts(SECOND), 1);
    assert_eq!(supervisor.recent_restarts(seconds(10)), 0);

    assert_eq!(
        supervisor.stopped(0, BAD.0, BAD.1, seconds(10)),
        restart(0, 0)
    );
    assert!(!supervisor.terminated());
}

#[test]
fn the_strategy_does_not_change_how_fast_the_intensity_is_spent() {
    let intensity = intensity(1, MINUTE);
    let mut wide: Supervisor<3, 16> = some(Supervisor::new(Strategy::OneForAll, intensity));
    let mut narrow: Supervisor<3, 16> = some(Supervisor::new(Strategy::OneForOne, intensity));

    assert!(wide.stopped(0, BAD.0, BAD.1, Duration::ZERO).is_ok());
    assert!(narrow.stopped(0, BAD.0, BAD.1, Duration::ZERO).is_ok());

    assert_eq!(wide.recent_restarts(Duration::ZERO), 1);
    assert_eq!(narrow.recent_restarts(Duration::ZERO), 1);
    assert_eq!(
        wide.stopped(0, BAD.0, BAD.1, SECOND),
        Ok(Decision::Escalate)
    );
    assert_eq!(
        narrow.stopped(0, BAD.0, BAD.1, SECOND),
        Ok(Decision::Escalate)
    );
}

/// The delay doubles while restarts pile up inside the window, holds at the
/// ceiling, and falls back to the base once the window has slid past them.
#[test]
fn the_restart_delay_backs_off_and_recovers() {
    let ms = Duration::from_millis;
    let mut tree: Supervisor<1, 16> = some(Supervisor::new(
        Strategy::OneForOne,
        intensity(8, ms(60_000)),
    ));

    let delays: [Duration; 5] = core::array::from_fn(|attempt| {
        let now = ms(attempt as u64);
        assert!(tree.stopped(0, BAD.0, BAD.1, now).is_ok());
        tree.restart_delay(ms(100), ms(800), now)
    });
    assert_eq!(delays, [ms(100), ms(200), ms(400), ms(800), ms(800)]);

    assert_eq!(tree.restart_delay(ms(100), ms(800), ms(120_000)), ms(100));
}

/// A base past the ceiling is still capped, and no doubling can overflow.
#[test]
fn the_restart_delay_never_outruns_its_ceiling() {
    let nanos = Duration::from_nanos;
    let mut tree: Supervisor<1, 128> = some(Supervisor::new(
        Strategy::OneForOne,
        intensity(127, Duration::MAX),
    ));
    for attempt in 0..40 {
        assert!(tree.stopped(0, BAD.0, BAD.1, nanos(attempt)).is_ok());
    }
    assert_eq!(
        tree.restart_delay(Duration::MAX, nanos(5), nanos(40)),
        nanos(5)
    );
    assert_eq!(
        tree.restart_delay(nanos(1), Duration::MAX, nanos(40)),
        nanos(1 << 39)
    );
    for attempt in 40..100 {
        assert!(tree.stopped(0, BAD.0, BAD.1, nanos(attempt)).is_ok());
    }
    assert_eq!(
        tree.restart_delay(nanos(1), Duration::MAX, nanos(100)),
        Duration::MAX
    );
}

#[test]
fn a_shape_that_could_decide_nothing_does_not_exist() {
    let window = intensity(3, MINUTE);
    assert!(Supervisor::<0, 8>::new(Strategy::OneForOne, window).is_none());
    assert!(Supervisor::<1, 3>::new(Strategy::OneForOne, window).is_none());
    assert!(Supervisor::<1, 0>::new(Strategy::OneForOne, intensity(0, MINUTE)).is_none());
    assert!(Supervisor::<1, 4>::new(Strategy::OneForOne, window).is_some());
    assert!(Supervisor::<1, 3>::restore(Strategy::OneForOne, window, [], MINUTE).is_none());
}
