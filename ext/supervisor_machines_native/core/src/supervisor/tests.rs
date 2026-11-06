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

/// The delay doubles while restarts pile up inside the window, holds at the
/// ceiling, and falls back to the base once the window has slid past them.
#[test]
fn the_restart_delay_backs_off_and_recovers() {
    const MS: u64 = 1_000_000;
    let mut tree: Supervisor<1, 16> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(8, 60_000 * MS));

    let delays: [u64; 5] = core::array::from_fn(|attempt| {
        let now = attempt as u64 * MS;
        tree.stopped(0, Policy::Permanent, Exit::Abnormal, now);
        tree.restart_delay_ns(100 * MS, 800 * MS, now)
    });
    assert_eq!(delays, [100 * MS, 200 * MS, 400 * MS, 800 * MS, 800 * MS]);

    assert_eq!(
        tree.restart_delay_ns(100 * MS, 800 * MS, 120_000 * MS),
        100 * MS
    );
}

/// A base past the ceiling is still capped, and no shift can overflow.
#[test]
fn the_restart_delay_never_outruns_its_ceiling() {
    let mut tree: Supervisor<1, 64> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(u32::MAX, u64::MAX));
    for attempt in 0..64 {
        tree.stopped(0, Policy::Permanent, Exit::Abnormal, attempt);
    }
    assert_eq!(tree.restart_delay_ns(u64::MAX, 5, 64), 5);
    assert_eq!(tree.restart_delay_ns(1, u64::MAX, 64), 1 << 63);
}

/// What one process persisted rebuilds the same intensity window in another.
#[test]
fn a_restored_history_spends_the_budget_the_first_process_already_spent() {
    let intensity = Intensity::new(2, MINUTE);
    let mut first: Supervisor<1, 8> = Supervisor::new(Strategy::OneForOne, intensity);
    first.stopped(0, Policy::Permanent, Exit::Abnormal, 10 * SECOND);
    first.stopped(0, Policy::Permanent, Exit::Abnormal, 20 * SECOND);
    assert!(first.history().eq([10 * SECOND, 20 * SECOND]));

    let mut second: Supervisor<1, 8> = Supervisor::restore(
        first.strategy(),
        first.intensity(),
        first.history(),
        25 * SECOND,
    );

    assert!(second.history().eq(first.history()));
    assert_eq!(second.recent_restarts(25 * SECOND), 2);
    assert_eq!(
        second.restart_delay_ns(SECOND, MINUTE, 25 * SECOND),
        2 * SECOND
    );
    assert_eq!(
        second.stopped(0, Policy::Permanent, Exit::Abnormal, 30 * SECOND),
        Decision::Escalate
    );
}

#[test]
fn restoring_drops_restarts_the_window_has_slid_past() {
    let saved = [SECOND, 2 * SECOND, 50 * SECOND, 55 * SECOND];
    let tree: Supervisor<1, 8> = Supervisor::restore(
        Strategy::OneForOne,
        Intensity::new(5, 10 * SECOND),
        saved,
        MINUTE,
    );

    assert!(tree.history().eq([50 * SECOND, 55 * SECOND]));
    assert_eq!(tree.recent_restarts(MINUTE), 2);
}

#[test]
fn restoring_more_than_fits_keeps_the_newest() {
    let tree: Supervisor<1, 4> = Supervisor::restore(
        Strategy::OneForOne,
        Intensity::new(3, MINUTE),
        (1..=6).map(|tick| tick * SECOND),
        10 * SECOND,
    );

    assert!(
        tree.history()
            .eq([3 * SECOND, 4 * SECOND, 5 * SECOND, 6 * SECOND])
    );
}

/// A restart another process performed counts the same as one granted here.
#[test]
fn an_external_restart_spends_the_budget() {
    let mut tree: Supervisor<1, 8> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(1, MINUTE));
    tree.record_external_restart(SECOND);

    assert!(!tree.terminated());
    assert_eq!(tree.recent_restarts(SECOND), 1);
    assert_eq!(tree.restart_delay_ns(SECOND, MINUTE, 2 * SECOND), SECOND);
    assert_eq!(
        tree.stopped(0, Policy::Permanent, Exit::Abnormal, 2 * SECOND),
        Decision::Escalate
    );
}

/// Two processes that each restarted the child merge into one window, in
/// order, and the same history absorbed twice counts nothing twice.
#[test]
fn absorbing_merges_in_order_without_counting_twice() {
    let intensity = Intensity::new(3, MINUTE);
    let mut mine: Supervisor<1, 8> = Supervisor::new(Strategy::OneForOne, intensity);
    mine.stopped(0, Policy::Permanent, Exit::Abnormal, 10 * SECOND);
    mine.stopped(0, Policy::Permanent, Exit::Abnormal, 30 * SECOND);

    let mut theirs: Supervisor<1, 8> = Supervisor::new(Strategy::OneForOne, intensity);
    theirs.record_external_restart(10 * SECOND);
    theirs.record_external_restart(20 * SECOND);

    mine.absorb(theirs.history(), 31 * SECOND);
    assert!(mine.history().eq([10 * SECOND, 20 * SECOND, 30 * SECOND]));

    mine.absorb(theirs.history(), 32 * SECOND);
    assert_eq!(mine.recent_restarts(32 * SECOND), 3);
    assert_eq!(
        mine.stopped(0, Policy::Permanent, Exit::Abnormal, 33 * SECOND),
        Decision::Escalate
    );
}

#[test]
fn absorbing_ignores_restarts_outside_the_window() {
    let mut tree: Supervisor<1, 8> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(2, 10 * SECOND));
    tree.absorb([SECOND, 2 * SECOND, 55 * SECOND], MINUTE);

    assert!(tree.history().eq([55 * SECOND]));
    assert!(matches!(
        tree.stopped(0, Policy::Permanent, Exit::Abnormal, MINUTE),
        Decision::Restart(_)
    ));
}

#[test]
fn a_full_history_lets_the_oldest_go_whichever_order_they_arrive() {
    let mut tree: Supervisor<1, 3> =
        Supervisor::new(Strategy::OneForOne, Intensity::new(2, MINUTE));
    tree.record_external_restart(5 * SECOND);
    tree.record_external_restart(3 * SECOND);
    tree.record_external_restart(9 * SECOND);
    tree.record_external_restart(7 * SECOND);
    tree.record_external_restart(SECOND);

    assert!(tree.history().eq([5 * SECOND, 7 * SECOND, 9 * SECOND]));
}
