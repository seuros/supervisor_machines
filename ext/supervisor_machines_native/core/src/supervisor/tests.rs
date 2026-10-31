use crate::{Decision, Exit, Intensity, Policy, Span, Strategy, Supervisor};

const SECOND: u64 = 1_000_000_000;
const MINUTE: u64 = 60 * SECOND;

fn tree<const CHILDREN: usize>(strategy: Strategy) -> Supervisor<CHILDREN, 64> {
    Supervisor::new(strategy, Intensity::new(100, MINUTE))
}

#[test]
fn one_for_one_restarts_only_the_child_that_died() {
    let mut supervisor = tree::<5>(Strategy::OneForOne);

    assert_eq!(
        supervisor.stopped(2, Policy::Permanent, Exit::Abnormal, 0),
        Decision::Restart(Span { first: 2, last: 2 })
    );
}

#[test]
fn one_for_all_restarts_every_child_whichever_one_died() {
    let mut supervisor = tree::<5>(Strategy::OneForAll);
    let whole = Span { first: 0, last: 4 };

    assert_eq!(
        supervisor.stopped(0, Policy::Permanent, Exit::Abnormal, 0),
        Decision::Restart(whole)
    );
    assert_eq!(
        supervisor.stopped(4, Policy::Permanent, Exit::Abnormal, SECOND),
        Decision::Restart(whole)
    );
}

#[test]
fn rest_for_one_restarts_the_child_and_everything_started_after_it() {
    let mut supervisor = tree::<5>(Strategy::RestForOne);

    assert_eq!(
        supervisor.stopped(2, Policy::Permanent, Exit::Abnormal, 0),
        Decision::Restart(Span { first: 2, last: 4 })
    );
    assert_eq!(
        supervisor.stopped(4, Policy::Permanent, Exit::Abnormal, SECOND),
        Decision::Restart(Span { first: 4, last: 4 })
    );
}

#[test]
fn a_temporary_child_is_never_restarted() {
    let mut supervisor = tree::<3>(Strategy::OneForAll);

    assert_eq!(
        supervisor.stopped(1, Policy::Temporary, Exit::Abnormal, 0),
        Decision::Ignore
    );
    assert_eq!(
        supervisor.stopped(1, Policy::Temporary, Exit::Normal, SECOND),
        Decision::Ignore
    );
}

#[test]
fn a_transient_child_comes_back_only_after_a_bad_exit() {
    let mut supervisor = tree::<3>(Strategy::OneForOne);

    assert_eq!(
        supervisor.stopped(1, Policy::Transient, Exit::Normal, 0),
        Decision::Ignore
    );
    assert_eq!(
        supervisor.stopped(1, Policy::Transient, Exit::Abnormal, SECOND),
        Decision::Restart(Span::single(1))
    );
}

#[test]
fn a_permanent_child_comes_back_even_from_a_clean_exit() {
    let mut supervisor = tree::<3>(Strategy::OneForOne);

    assert_eq!(
        supervisor.stopped(1, Policy::Permanent, Exit::Normal, 0),
        Decision::Restart(Span::single(1))
    );
}

#[test]
fn an_ignored_stop_does_not_spend_the_restart_budget() {
    let mut supervisor: Supervisor<3, 8> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(1, MINUTE));

    for tick in 0..50 {
        supervisor.stopped(0, Policy::Temporary, Exit::Abnormal, tick * SECOND);
    }

    assert_eq!(supervisor.recent_restarts(50 * SECOND), 0);
    assert!(!supervisor.terminated());
}

#[test]
fn exceeding_the_intensity_escalates_and_stays_escalated() {
    let mut supervisor: Supervisor<3, 8> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(2, MINUTE));

    assert!(matches!(
        supervisor.stopped(0, Policy::Permanent, Exit::Abnormal, 0),
        Decision::Restart(_)
    ));
    assert!(matches!(
        supervisor.stopped(0, Policy::Permanent, Exit::Abnormal, SECOND),
        Decision::Restart(_)
    ));
    assert_eq!(
        supervisor.stopped(0, Policy::Permanent, Exit::Abnormal, 2 * SECOND),
        Decision::Escalate
    );

    assert!(supervisor.terminated());
    assert_eq!(
        supervisor.stopped(1, Policy::Temporary, Exit::Normal, 3 * SECOND),
        Decision::Escalate
    );
}

#[test]
fn restarts_outside_the_window_stop_counting() {
    let mut supervisor: Supervisor<3, 8> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(1, 5 * SECOND));
    supervisor.stopped(0, Policy::Permanent, Exit::Abnormal, 0);

    assert_eq!(supervisor.recent_restarts(SECOND), 1);
    assert_eq!(supervisor.recent_restarts(10 * SECOND), 0);

    assert!(matches!(
        supervisor.stopped(0, Policy::Permanent, Exit::Abnormal, 10 * SECOND),
        Decision::Restart(_)
    ));
    assert!(!supervisor.terminated());
}

#[test]
fn the_strategy_does_not_change_how_fast_the_intensity_is_spent() {
    let intensity = Intensity::new(1, MINUTE);
    let mut wide: Supervisor<3, 16> = Supervisor::new(Strategy::OneForAll, intensity);
    let mut narrow: Supervisor<3, 16> = Supervisor::new(Strategy::OneForOne, intensity);

    wide.stopped(0, Policy::Permanent, Exit::Abnormal, 0);
    narrow.stopped(0, Policy::Permanent, Exit::Abnormal, 0);

    assert_eq!(wide.recent_restarts(0), narrow.recent_restarts(0));
    assert_eq!(wide.recent_restarts(0), 1);

    assert_eq!(
        wide.stopped(0, Policy::Permanent, Exit::Abnormal, SECOND),
        Decision::Escalate
    );
    assert_eq!(
        narrow.stopped(0, Policy::Permanent, Exit::Abnormal, SECOND),
        Decision::Escalate
    );
}
