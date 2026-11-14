//! Ruby bindings for the `supervisor-machines` decision machine.
//!
//! The core decides and nothing else: this crate converts Ruby symbols and
//! `Data` values into its types, hands back every answer as a symbol or a
//! `Data` value, and leaves spawning, joining and killing to
//! `SupervisorMachines::Supervisor`.
//!
//! Every instant and span crosses as Integer nanoseconds: a non-Integer is a
//! `TypeError`, a negative or oversized one a `RangeError`.

mod decider;
mod init;

use magnus::{Error, IntoValue, RArray, RClass, RModule, Ruby, Value, prelude::*};
use matryona_machines::ruby::{
    Class, Registry, Symbols, duration, exact_fields, integer, nanoseconds, symbol,
};
use supervisor_machines::{Decision, Exit, Intensity, Policy, Recording, Refusal, Span, Strategy};

matryona_machines::symbols! {
    /// Every [`Strategy`], in the order the core declares them.
    pub(crate) Strategies for Strategy as "strategy" {
        OneForOne => "one_for_one",
        OneForAll => "one_for_all",
        RestForOne => "rest_for_one",
    }
}

matryona_machines::symbols! {
    /// Every [`Policy`], in the order the core declares them.
    pub(crate) Policies for Policy as "policy" {
        Permanent => "permanent",
        Transient => "transient",
        Temporary => "temporary",
    }
}

matryona_machines::symbols! {
    /// Every [`Exit`], in the order the core declares them.
    pub(crate) Exits for Exit as "exit" {
        Normal => "normal",
        Abnormal => "abnormal",
    }
}

matryona_machines::symbols! {
    /// The core's refusals.
    pub(crate) Refusals for Refusal as "a refusal" {
        UnknownChild => "unknown_child",
    }
}

matryona_machines::symbols! {
    /// What `record_external_restart` answers.
    pub(crate) Recordings for Recording as "a recording" {
        Recorded => "recorded",
        Outdated => "outdated",
    }
}

pub(crate) fn module(ruby: &Ruby) -> Result<RModule, Error> {
    ruby.define_module("SupervisorMachines")
}

/// The gem's `Data` classes, resolved once at load (`values.rb` defines them
/// before the extension is required) and pinned: the exact-class check
/// compares against these, and no answer looks a class up by name after the
/// core decided.
struct Classes {
    intensity: Class,
    span: Class,
    restart: Class,
}

static CLASSES: Registry<Classes> = Registry::new("supervisor_machines");

fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get(ruby)
}

pub(crate) fn resolve_classes(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = |name| Class::resolve(ruby, module, name);
    let classes = Classes {
        intensity: class("Intensity")?,
        span: class("Span")?,
        restart: class("Restart")?,
    };
    CLASSES.set(ruby, classes).map(drop)
}

fn checked_intensity(ruby: &Ruby, restarts: Value, window: Value) -> Result<Intensity, Error> {
    let (restarts, window) = (integer(restarts, "restarts")?, duration(window, "window")?);
    Intensity::new(restarts, window).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "an intensity window must be longer than zero",
        )
    })
}

fn checked_span(ruby: &Ruby, first: Value, last: Value) -> Result<Span, Error> {
    let (first, last) = (integer(first, "first")?, integer(last, "last")?);
    Span::new(first, last).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            format!("a span cannot end at {last} before it starts at {first}"),
        )
    })
}

// A value read back is an instance of exactly the gem's class (a subclass's
// constant leaks past the seal, so `is_a?` is not enough; an opened singleton
// class is fine), its fields read from the struct itself so a redefined
// reader is never called, and it is revalidated through the core.

/// A `SupervisorMachines::Intensity`, revalidated through the core.
pub(crate) fn intensity_from(ruby: &Ruby, value: Value) -> Result<Intensity, Error> {
    let class = classes(ruby)?.intensity.get(ruby);
    let message = "expected a SupervisorMachines::Intensity";
    let [restarts, window] = exact_fields(ruby, value, class, message)?;
    checked_intensity(ruby, restarts, window)
}

/// A `SupervisorMachines::Span`, revalidated through the core.
fn span_from(ruby: &Ruby, value: Value) -> Result<Span, Error> {
    let class = classes(ruby)?.span.get(ruby);
    let [first, last] = exact_fields(ruby, value, class, "expected a SupervisorMachines::Span")?;
    checked_span(ruby, first, last)
}

pub(crate) fn intensity_value(ruby: &Ruby, intensity: Intensity) -> Result<Value, Error> {
    let window = nanoseconds(ruby, intensity.window());
    let class = classes(ruby)?.intensity.get(ruby);
    class.funcall("new", (intensity.restarts(), window))
}

/// The classes a [`Decision`] is built from, taken before the core is asked
/// so that nothing after it can fail on a lookup.
#[derive(Clone, Copy)]
pub(crate) struct DecisionClasses {
    span: RClass,
    restart: RClass,
}

impl DecisionClasses {
    pub(crate) fn fetch(ruby: &Ruby) -> Result<Self, Error> {
        let classes = classes(ruby)?;
        Ok(Self {
            span: classes.span.get(ruby),
            restart: classes.restart.get(ruby),
        })
    }

    pub(crate) fn span(self, span: Span) -> Result<Value, Error> {
        self.span.funcall("new", (span.first(), span.last()))
    }

    /// `:ignore`, a `Restart`, `:escalate` or `:already_escalated`.
    pub(crate) fn decision(self, ruby: &Ruby, decision: Decision) -> Result<Value, Error> {
        match decision {
            Decision::Ignore => Ok(symbol(ruby, "ignore").as_value()),
            Decision::Restart(span) => self.restart.funcall("new", (self.span(span)?,)),
            Decision::Escalate => Ok(symbol(ruby, "escalate").as_value()),
            Decision::AlreadyEscalated => Ok(symbol(ruby, "already_escalated").as_value()),
        }
    }
}

/// What `Intensity.new` stores: the validated restart count and window.
pub(crate) fn intensity_hook(ruby: &Ruby, restarts: Value, window: Value) -> Result<Value, Error> {
    let intensity = checked_intensity(ruby, restarts, window)?;
    Ok((intensity.restarts(), nanoseconds(ruby, intensity.window())).into_value_with(ruby))
}

/// What `Span.new` stores: the validated ends.
pub(crate) fn span_hook(ruby: &Ruby, first: Value, last: Value) -> Result<(usize, usize), Error> {
    checked_span(ruby, first, last).map(|span| (span.first(), span.last()))
}

/// What `Restart.new` stores: a `SupervisorMachines::Span`, checked for its
/// exact class and revalidated through the core here, not in Ruby.
pub(crate) fn restart_hook(ruby: &Ruby, span: Value) -> Result<Value, Error> {
    span_from(ruby, span).map(|_| span)
}

/// A policy symbol the core knows, or `TypeError`/`RangeError`.
pub(crate) fn policy_hook(ruby: &Ruby, value: Value) -> Result<Value, Error> {
    Policies::parse(ruby, value).map(|_| value)
}

/// A strategy symbol the core knows, or `TypeError`/`RangeError`.
pub(crate) fn strategy_hook(ruby: &Ruby, value: Value) -> Result<Value, Error> {
    Strategies::parse(ruby, value).map(|_| value)
}

/// A frozen array of `items`, in order.
pub(crate) fn frozen<T: IntoValue>(
    ruby: &Ruby,
    items: impl ExactSizeIterator<Item = T>,
) -> Result<RArray, Error> {
    let array = ruby.ary_new_capa(items.len());
    for item in items {
        array.push(item)?;
    }
    array.freeze();
    Ok(array)
}

/// A frozen array of the symbols naming a closed set, in table order.
pub(crate) fn names<T: Symbols>(ruby: &Ruby) -> Result<RArray, Error> {
    frozen(ruby, T::ALL.iter().map(|&each| T::symbol(ruby, each)))
}
