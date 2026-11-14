//! A refusal changes nothing: every one is compared against a clone.

use core::assert_matches;
use core::time::Duration;

use super::{BAD, MINUTE, SECOND, small, tree};
use crate::{Decision, Exit, Policy, Refusal, Strategy};

/// The defect this guards against: the core used to answer
/// `Restart(Span { first: 7, last: 2 })` for a child it does not own.
#[test]
fn a_child_outside_the_tree_is_refused() {
    for strategy in [
        Strategy::OneForOne,
        Strategy::OneForAll,
        Strategy::RestForOne,
    ] {
        let mut supervisor = tree::<3>(strategy);
        assert!(supervisor.stopped(0, BAD.0, BAD.1, SECOND).is_ok());
        let before = supervisor;

        for index in [3, 7, usize::MAX] {
            for (policy, exit) in [BAD, (Policy::Temporary, Exit::Normal)] {
                assert_eq!(
                    supervisor.stopped(index, policy, exit, MINUTE),
                    Err(Refusal::UnknownChild)
                );
                assert_eq!(supervisor, before);
            }
            assert_eq!(supervisor.span(index), None);
        }
    }
}

/// A malformed call is refused before the supervisor says it is terminated.
#[test]
fn a_child_outside_a_terminated_tree_is_still_refused() {
    let mut supervisor = small::<4>(0, MINUTE);
    assert_eq!(
        supervisor.stopped(0, BAD.0, BAD.1, SECOND),
        Ok(Decision::Escalate)
    );
    let before = supervisor;

    assert_eq!(
        supervisor.stopped(3, BAD.0, BAD.1, MINUTE),
        Err(Refusal::UnknownChild)
    );
    assert_eq!(supervisor, before);
}

/// A refusal records no clock, so the refused instant constrains nothing.
#[test]
fn a_refused_instant_is_not_remembered() {
    let mut supervisor = small::<4>(1, MINUTE);
    assert_eq!(
        supervisor.stopped(9, BAD.0, BAD.1, MINUTE),
        Err(Refusal::UnknownChild)
    );
    assert_eq!(supervisor.history().count(), 0);
    assert_matches!(
        supervisor.stopped(0, BAD.0, BAD.1, Duration::ZERO),
        Ok(Decision::Restart(_))
    );
}
