#![cfg_attr(not(feature = "std"), no_std)]

//! Supervision decisions, in the OTP sense.
//!
//! Which children restart after one dies, and whether that has happened too
//! often to keep trying. The caller owns the threads, fibers or processes.

/// Compiles and runs the README's examples as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub mod supervisor;

pub use supervisor::Supervisor;

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

/// What the supervisor should do about a child that stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Leave it stopped. Nothing restarts.
    Ignore,
    /// Restart this inclusive range of children, in index order.
    Restart(Span),
    /// Restarting is not working. The supervisor must terminate its remaining
    /// children and let its own supervisor deal with it.
    Escalate,
}

/// An inclusive range of child indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Lowest index, restarted first.
    pub first: usize,
    /// Highest index, restarted last.
    pub last: usize,
}

impl Span {
    #[must_use]
    pub const fn single(index: usize) -> Self {
        Self {
            first: index,
            last: index,
        }
    }

    /// Children in the span. Always at least one.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.last - self.first + 1
    }

    /// Never true. A span always covers the child that died.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    #[must_use]
    pub const fn contains(&self, index: usize) -> bool {
        self.first <= index && index <= self.last
    }
}

/// How many restarts within how long before a supervisor gives up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intensity {
    /// Restarts tolerated inside the window.
    pub restarts: u32,
    /// Length of the window, in nanoseconds.
    pub window_ns: u64,
}

/// Restarts OTP tolerates by default before escalating.
pub const DEFAULT_RESTARTS: u32 = 1;

/// The window OTP measures those restarts over, five seconds.
pub const DEFAULT_WINDOW_NS: u64 = 5_000_000_000;

impl Intensity {
    #[must_use]
    pub const fn new(restarts: u32, window_ns: u64) -> Self {
        Self {
            restarts,
            window_ns,
        }
    }
}

impl Default for Intensity {
    fn default() -> Self {
        Self::new(DEFAULT_RESTARTS, DEFAULT_WINDOW_NS)
    }
}

#[cfg(test)]
mod tests;
