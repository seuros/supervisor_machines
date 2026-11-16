#![no_std]
#![cfg_attr(not(test), deny(clippy::indexing_slicing))]

//! Supervision decisions, in the OTP sense.
//!
//! Which children restart after one dies, and whether that has happened too
//! often to keep trying. The caller owns the threads, fibers or processes.
//!
//! Time is [`core::time::Duration`] since an epoch the caller picks, for
//! instants and spans alike. The core reads no clock.

/// Compiles and runs the README's examples as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod supervisor;

use core::fmt;
use core::time::Duration;

pub use supervisor::Supervisor;

/// Why a call was not honoured. A refused call changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The child index is not below `CHILDREN`: the supervisor owns no such
    /// child, so there is nothing to decide.
    UnknownChild,
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownChild => "the supervisor owns no child at that index",
        })
    }
}

impl core::error::Error for Refusal {}

/// Which siblings a failure takes down with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Restart only the child that died.
    OneForOne,
    /// Restart every child.
    OneForAll,
    /// Restart the child that died and everything started after it.
    RestForOne,
}

/// When a child is worth restarting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Always bring it back, even after a clean exit.
    Permanent,
    /// Bring it back only if it died badly. A clean exit means it was done.
    Transient,
    /// Never bring it back.
    Temporary,
}

/// How a child's run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// It finished its work.
    Normal,
    /// It raised, was killed, or vanished.
    Abnormal,
}

/// What the supervisor should do about a child that stopped. Every variant is
/// an honoured answer: the question was well formed and this is the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Leave it stopped. Nothing restarts and nothing was recorded.
    Ignore,
    /// Restart this inclusive range of children, in index order. The restart
    /// was recorded at the instant supplied.
    Restart(Span),
    /// Restarting is not working: this failure spent the intensity. The
    /// restart was recorded (unless a full history of newer restarts left no
    /// room for it, which is itself past the intensity), the supervisor is
    /// now terminated, and it must
    /// take its remaining children down and let its own supervisor decide.
    Escalate,
    /// The supervisor had already escalated. Nothing was recorded: the answer
    /// holds whatever the time, and a supervisor that gave up stays given up.
    AlreadyEscalated,
}

/// What [`Supervisor::record_external_restart`] did with the restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recording {
    /// The restart is now in the history.
    Recorded,
    /// The history is full of newer restarts, which already decide every
    /// question this one could have changed. Nothing changed.
    Outdated,
}

/// An inclusive range of child indices. It always covers at least one child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    first: usize,
    last: usize,
}

impl Span {
    /// The children `first..=last`, or `None` when `first` is past `last`.
    #[must_use]
    pub const fn new(first: usize, last: usize) -> Option<Self> {
        if first > last {
            return None;
        }
        Some(Self { first, last })
    }

    /// The one child at `index`.
    #[must_use]
    pub const fn single(index: usize) -> Self {
        Self {
            first: index,
            last: index,
        }
    }

    /// Lowest index, restarted first.
    #[must_use]
    pub const fn first(&self) -> usize {
        self.first
    }

    /// Highest index, restarted last.
    #[must_use]
    pub const fn last(&self) -> usize {
        self.last
    }

    /// Children in the span. Always at least one.
    #[must_use]
    pub const fn len(&self) -> usize {
        // `first <= last` by construction.
        (self.last - self.first).saturating_add(1)
    }

    /// Never true. A span always covers the child that died.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Whether the child at `index` is in the span.
    #[must_use]
    pub const fn contains(&self, index: usize) -> bool {
        self.first <= index && index <= self.last
    }
}

/// How many restarts within how long before a supervisor gives up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intensity {
    restarts: u32,
    window: Duration,
}

/// Restarts OTP tolerates by default before escalating.
pub const DEFAULT_RESTARTS: u32 = 1;

/// The window OTP measures those restarts over, five seconds.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(5);

impl Intensity {
    /// `restarts` tolerated inside `window`, or `None` when the window is
    /// zero: a restart cannot be measured over no time at all.
    #[must_use]
    pub const fn new(restarts: u32, window: Duration) -> Option<Self> {
        if window.is_zero() {
            return None;
        }
        Some(Self { restarts, window })
    }

    /// Restarts tolerated inside the window.
    #[must_use]
    pub const fn restarts(&self) -> u32 {
        self.restarts
    }

    /// Length of the window.
    #[must_use]
    pub const fn window(&self) -> Duration {
        self.window
    }
}

impl Default for Intensity {
    fn default() -> Self {
        Self {
            restarts: DEFAULT_RESTARTS,
            window: DEFAULT_WINDOW,
        }
    }
}

#[cfg(test)]
mod tests;
