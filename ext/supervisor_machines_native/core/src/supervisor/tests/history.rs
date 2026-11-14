//! Sharing one restart history across processes.

extern crate std;

use core::assert_matches;
use core::time::Duration;
use std::vec::Vec;

use matryona_machines::testing::unchanged;

use super::{BAD, MINUTE, SECOND, intensity, seconds, small, some};
use crate::{Decision, Recording, Strategy, Supervisor};

/// What one process persisted rebuilds the same intensity window in another.
#[test]
fn a_restored_history_spends_the_budget_the_first_process_already_spent() {
    let mut first = small::<8>(2, MINUTE);
    assert!(first.stopped(0, BAD.0, BAD.1, seconds(10)).is_ok());
    assert!(first.stopped(0, BAD.0, BAD.1, seconds(20)).is_ok());
    assert!(first.history().eq([seconds(10), seconds(20)]));

    let mut second: Supervisor<3, 8> = some(Supervisor::restore(
        first.strategy(),
        first.intensity(),
        first.history(),
        seconds(25),
    ));

    assert_eq!(second, first);
    assert_eq!(second.recent_restarts(seconds(25)), 2);
    assert_eq!(
        second.restart_delay(SECOND, MINUTE, seconds(25)),
        seconds(2)
    );
    assert_eq!(
        second.stopped(0, BAD.0, BAD.1, seconds(30)),
        Ok(Decision::Escalate)
    );
}

/// Escalation is each process's own decision: it is not in the history.
#[test]
fn a_restored_supervisor_is_not_terminated() {
    let mut first = small::<8>(0, MINUTE);
    assert_eq!(
        first.stopped(0, BAD.0, BAD.1, SECOND),
        Ok(Decision::Escalate)
    );

    let second: Supervisor<3, 8> = some(Supervisor::restore(
        Strategy::OneForOne,
        first.intensity(),
        first.history(),
        SECOND,
    ));
    assert!(!second.terminated());
    assert!(second.history().eq(first.history()));
}

/// Saved state is a multiset: two restarts at one instant restore as two, so
/// the budget they spent is still spent.
#[test]
fn restoring_keeps_two_restarts_at_one_instant() {
    let mut first = small::<8>(2, MINUTE);
    first.record_external_restart(seconds(10));
    first.record_external_restart(seconds(10));
    assert!(first.history().eq([seconds(10), seconds(10)]));

    let mut second: Supervisor<3, 8> = some(Supervisor::restore(
        first.strategy(),
        first.intensity(),
        first.history(),
        seconds(20),
    ));

    assert_eq!(second, first);
    assert_eq!(second.recent_restarts(seconds(20)), 2);
    assert_eq!(
        second.stopped(0, BAD.0, BAD.1, seconds(20)),
        Ok(Decision::Escalate)
    );
}

#[test]
fn restoring_drops_restarts_the_window_has_slid_past() {
    let saved = [SECOND, seconds(2), seconds(50), seconds(55)];
    let tree: Supervisor<1, 8> = some(Supervisor::restore(
        Strategy::OneForOne,
        intensity(5, seconds(10)),
        saved,
        MINUTE,
    ));

    assert!(tree.history().eq([seconds(50), seconds(55)]));
    assert_eq!(tree.recent_restarts(MINUTE), 2);
}

#[test]
fn restoring_more_than_fits_keeps_the_newest() {
    let tree: Supervisor<1, 4> = some(Supervisor::restore(
        Strategy::OneForOne,
        intensity(3, MINUTE),
        (1..=6).map(seconds),
        seconds(10),
    ));

    assert!(tree.history().eq((3..=6).map(seconds)));
}

/// A restart another process performed counts the same as one granted here.
#[test]
fn an_external_restart_spends_the_budget() {
    let mut tree = small::<8>(1, MINUTE);
    assert_eq!(tree.record_external_restart(SECOND), Recording::Recorded);

    assert!(!tree.terminated());
    assert_eq!(tree.recent_restarts(SECOND), 1);
    assert_eq!(tree.restart_delay(SECOND, MINUTE, seconds(2)), SECOND);
    assert_eq!(
        tree.stopped(0, BAD.0, BAD.1, seconds(2)),
        Ok(Decision::Escalate)
    );
}

/// Two restarts in the same nanosecond are still two restarts.
#[test]
fn an_external_restart_at_an_instant_already_held_counts_again() {
    let mut tree = small::<8>(5, MINUTE);
    assert_eq!(tree.record_external_restart(SECOND), Recording::Recorded);
    assert_eq!(tree.record_external_restart(SECOND), Recording::Recorded);
    assert_eq!(tree.recent_restarts(SECOND), 2);
}

/// Two processes that each restarted the child merge into one window, in
/// order, and the same history absorbed twice counts nothing twice.
#[test]
fn absorbing_merges_in_order_without_counting_twice() {
    let mut mine = small::<8>(3, MINUTE);
    assert!(mine.stopped(0, BAD.0, BAD.1, seconds(10)).is_ok());
    assert!(mine.stopped(0, BAD.0, BAD.1, seconds(30)).is_ok());

    let mut theirs = small::<8>(3, MINUTE);
    theirs.record_external_restart(seconds(10));
    theirs.record_external_restart(seconds(20));
    let shared: Vec<Duration> = theirs.history().collect();

    assert_eq!(mine.absorb(&shared, seconds(31)), 1);
    assert!(mine.history().eq([seconds(10), seconds(20), seconds(30)]));

    assert_eq!(unchanged(&mut mine, |m| m.absorb(&shared, seconds(32))), 0);
    assert_eq!(mine.recent_restarts(seconds(32)), 3);
    assert_eq!(
        mine.stopped(0, BAD.0, BAD.1, seconds(33)),
        Ok(Decision::Escalate)
    );
}

#[test]
fn absorbing_ignores_restarts_outside_the_window() {
    let mut tree = small::<8>(2, seconds(10));
    assert_eq!(tree.absorb(&[SECOND, seconds(2), seconds(55)], MINUTE), 1);

    assert!(tree.history().eq([seconds(55)]));
    assert_matches!(
        tree.stopped(0, BAD.0, BAD.1, MINUTE),
        Ok(Decision::Restart(_))
    );
}

#[test]
fn a_full_history_lets_the_oldest_go_whichever_order_they_arrive() {
    let mut tree = small::<3>(2, MINUTE);
    for at in [5, 3, 9, 7] {
        assert_eq!(
            tree.record_external_restart(seconds(at)),
            Recording::Recorded
        );
    }
    assert!(tree.history().eq([seconds(5), seconds(7), seconds(9)]));

    // Older than everything a full history holds: honoured, nothing changed.
    assert_eq!(
        unchanged(&mut tree, |t| t.record_external_restart(SECOND)),
        Recording::Outdated
    );
    assert_eq!(
        unchanged(&mut tree, |t| t
            .absorb(&[SECOND, Duration::ZERO], seconds(9))),
        0
    );
}

/// Absorbing is a max-multiplicity merge: each instant ends up held as many
/// times as the more generous side holds it, so another process's two
/// restarts in one nanosecond stay two, and a repeat changes nothing.
#[test]
fn absorbing_keeps_the_larger_count_of_each_instant() {
    let mut mine = small::<8>(5, MINUTE);
    mine.record_external_restart(seconds(10));

    assert_eq!(mine.absorb(&[seconds(10), seconds(10)], seconds(11)), 1);
    assert!(mine.history().eq([seconds(10), seconds(10)]));

    assert_eq!(
        unchanged(&mut mine, |m| m
            .absorb(&[seconds(10), seconds(10)], seconds(12))),
        0
    );
    assert_eq!(
        unchanged(&mut mine, |m| m.absorb(&[seconds(10)], seconds(13))),
        0
    );
    assert_eq!(mine.recent_restarts(seconds(13)), 2);
}

/// The merge does not depend on which side absorbs which, nor on the order
/// the absorbed history lists its instants in.
#[test]
fn absorbing_is_commutative_and_order_blind() {
    let left = [seconds(10), seconds(30), seconds(10), seconds(20)];
    let right = [seconds(20), seconds(10), seconds(20), seconds(40)];

    let mut a = small::<8>(7, MINUTE);
    assert_eq!(a.absorb(&left, seconds(41)), 4);
    let mut b = small::<8>(7, MINUTE);
    assert_eq!(b.absorb(&right, seconds(41)), 4);

    let a_history: Vec<Duration> = a.history().collect();
    let b_history: Vec<Duration> = b.history().collect();
    assert_eq!(a.absorb(&b_history, seconds(41)), 2);
    assert_eq!(b.absorb(&a_history, seconds(41)), 2);

    assert_eq!(a, b);
    let merged = [10, 10, 20, 20, 30, 40].map(seconds);
    assert!(a.history().eq(merged));
}

/// A merge past `HISTORY` keeps the newest, as recording does, and answers
/// only what is still held; repeating it changes nothing.
#[test]
fn absorbing_past_a_full_history_keeps_the_newest() {
    let mut tree = small::<3>(2, MINUTE);
    tree.record_external_restart(seconds(5));
    tree.record_external_restart(seconds(5));
    tree.record_external_restart(seconds(7));

    // A third 5 would only swap an equal 5 out: nothing added.
    assert_eq!(
        unchanged(&mut tree, |t| {
            t.absorb(&[seconds(5), seconds(5), seconds(5)], seconds(9))
        }),
        0
    );

    // 9 and 8 push both 5s out; the 6 is older than all three left.
    let theirs = [seconds(9), seconds(6), seconds(8)];
    assert_eq!(tree.absorb(&theirs, seconds(9)), 2);
    assert!(tree.history().eq([seconds(7), seconds(8), seconds(9)]));

    let after = tree;
    assert_eq!(tree.absorb(&theirs, seconds(9)), 0);
    assert_eq!(tree, after);
}
