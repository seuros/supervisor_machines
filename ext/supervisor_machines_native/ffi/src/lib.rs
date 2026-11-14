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

use std::sync::OnceLock;
use std::time::Duration;

use magnus::rb_sys::AsRawValue;
use magnus::value::Opaque;
use magnus::{
    Error, Integer, IntoValue, RArray, RClass, RModule, Ruby, Symbol, TryConvert, Value, function,
    prelude::*,
};
use supervisor_machines::{
    DEFAULT_RESTARTS, DEFAULT_WINDOW, Decision, Exit, Intensity, Policy, Recording, Refusal, Span,
    Strategy,
};

/// A whole, nonnegative number of things: a child count, a child index, a
/// restart count, a nanosecond reading.
///
/// magnus reads an integer through Ruby's `NUM2*` family, which truncates a
/// `Float` towards zero instead of refusing it, so anything but an `Integer`
/// is a `TypeError`. A negative `Integer` is a `RangeError` here, and one past
/// the target width comes back as the `RangeError` magnus raises for it.
pub(crate) struct Whole<T>(pub(crate) T);

impl<T: TryConvert> TryConvert for Whole<T> {
    fn try_convert(value: Value) -> Result<Self, Error> {
        let ruby = Ruby::get_with(value);
        let Some(integer) = Integer::from_value(value) else {
            return Err(Error::new(
                ruby.exception_type_error(),
                format!(
                    "no implicit conversion of {} into Integer: {}",
                    value.class(),
                    value.inspect()
                ),
            ));
        };
        if integer.funcall::<_, _, bool>("negative?", ())? {
            return Err(Error::new(
                ruby.exception_range_error(),
                format!("{} is negative", value.inspect()),
            ));
        }
        T::try_convert(value).map(Self)
    }
}

/// An instant or span given as Integer nanoseconds.
pub(crate) struct Nanos(pub(crate) Duration);

impl TryConvert for Nanos {
    fn try_convert(value: Value) -> Result<Self, Error> {
        Whole::<u64>::try_convert(value).map(|Whole(nanos)| Self(Duration::from_nanos(nanos)))
    }
}

/// A duration the core answered, as Integer nanoseconds. Never raises: every
/// `Duration` fits a Ruby Integer.
pub(crate) fn nanos(ruby: &Ruby, duration: Duration) -> Value {
    ruby.integer_from_u128(duration.as_nanos()).as_value()
}

/// Every [`Strategy`], in the order the core declares them.
pub(crate) const STRATEGIES: [(&str, Strategy); 3] = [
    ("one_for_one", Strategy::OneForOne),
    ("one_for_all", Strategy::OneForAll),
    ("rest_for_one", Strategy::RestForOne),
];

/// Every [`Policy`], in the order the core declares them.
pub(crate) const POLICIES: [(&str, Policy); 3] = [
    ("permanent", Policy::Permanent),
    ("transient", Policy::Transient),
    ("temporary", Policy::Temporary),
];

/// Every [`Exit`], in the order the core declares them.
pub(crate) const EXITS: [(&str, Exit); 2] =
    [("normal", Exit::Normal), ("abnormal", Exit::Abnormal)];

/// Resolves a symbol against one of the closed sets above: `TypeError` for
/// anything but a `Symbol`, `RangeError` for a symbol outside the set.
fn member<T: Copy>(ruby: &Ruby, kind: &str, value: Value, set: &[(&str, T)]) -> Result<T, Error> {
    let Some(symbol) = Symbol::from_value(value) else {
        return Err(Error::new(
            ruby.exception_type_error(),
            format!("{kind} must be a Symbol, got {}", value.inspect()),
        ));
    };
    let name = symbol.name()?;
    set.iter()
        .find(|(known, _)| *known == name)
        .map(|&(_, member)| member)
        .ok_or_else(|| {
            Error::new(
                ruby.exception_range_error(),
                format!("unknown {kind}: {}", value.inspect()),
            )
        })
}

pub(crate) fn strategy(ruby: &Ruby, value: Value) -> Result<Strategy, Error> {
    member(ruby, "strategy", value, &STRATEGIES)
}

pub(crate) fn policy(ruby: &Ruby, value: Value) -> Result<Policy, Error> {
    member(ruby, "policy", value, &POLICIES)
}

pub(crate) fn exit(ruby: &Ruby, value: Value) -> Result<Exit, Error> {
    member(ruby, "exit", value, &EXITS)
}

pub(crate) const fn strategy_name(strategy: Strategy) -> &'static str {
    match strategy {
        Strategy::OneForOne => "one_for_one",
        Strategy::OneForAll => "one_for_all",
        Strategy::RestForOne => "rest_for_one",
    }
}

pub(crate) const fn refusal_name(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::UnknownChild => "unknown_child",
    }
}

pub(crate) const fn recording_name(recording: Recording) -> &'static str {
    match recording {
        Recording::Recorded => "recorded",
        Recording::Outdated => "outdated",
    }
}

fn module(ruby: &Ruby) -> Result<RModule, Error> {
    ruby.define_module("SupervisorMachines")
}

/// The gem's `Data` classes, resolved once at load (`values.rb` defines them
/// before the extension is required): the exact-class check compares against
/// these, and no answer looks a class up by name after the core decided.
struct Classes {
    intensity: Opaque<RClass>,
    span: Opaque<RClass>,
    restart: Opaque<RClass>,
}

static CLASSES: OnceLock<Classes> = OnceLock::new();

fn classes(ruby: &Ruby) -> Result<&'static Classes, Error> {
    CLASSES.get().ok_or_else(|| {
        Error::new(
            ruby.exception_runtime_error(),
            "load supervisor_machines, not the extension alone",
        )
    })
}

fn resolve_classes(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    // Held past this call: pinned so GC compaction cannot move them.
    let class = |name: &str| {
        let class = module.const_get::<_, RClass>(name)?;
        ruby.gc_register_mark_object(class);
        Ok::<_, Error>(Opaque::from(class))
    };
    let classes = Classes {
        intensity: class("Intensity")?,
        span: class("Span")?,
        restart: class("Restart")?,
    };
    CLASSES.set(classes).map_err(|_| {
        Error::new(
            ruby.exception_runtime_error(),
            "the extension was loaded twice",
        )
    })
}

/// `value` when its class is exactly `class`, `TypeError` otherwise: a
/// subclass's constant leaks past the seal, so `is_a?` is not enough. The
/// classes are compared by identity, which no Ruby method can fake.
fn instance(ruby: &Ruby, value: Value, class: Opaque<RClass>, name: &str) -> Result<Value, Error> {
    if value.class().as_raw() == ruby.get_inner(class).as_raw() {
        return Ok(value);
    }
    Err(Error::new(
        ruby.exception_type_error(),
        format!(
            "expected a SupervisorMachines::{name}, got {}",
            value.inspect()
        ),
    ))
}

fn checked_intensity(ruby: &Ruby, restarts: Value, window: Value) -> Result<Intensity, Error> {
    let (Whole(restarts), Nanos(window)) =
        (Whole::try_convert(restarts)?, Nanos::try_convert(window)?);
    Intensity::new(restarts, window).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            "an intensity window must be longer than zero",
        )
    })
}

fn checked_span(ruby: &Ruby, first: Value, last: Value) -> Result<Span, Error> {
    let (Whole(first), Whole(last)) = (Whole::try_convert(first)?, Whole::try_convert(last)?);
    Span::new(first, last).ok_or_else(|| {
        Error::new(
            ruby.exception_range_error(),
            format!("a span cannot end at {last} before it starts at {first}"),
        )
    })
}

/// A `SupervisorMachines::Intensity`, revalidated through the core.
pub(crate) fn intensity_from(ruby: &Ruby, value: Value) -> Result<Intensity, Error> {
    let value = instance(ruby, value, classes(ruby)?.intensity, "Intensity")?;
    checked_intensity(
        ruby,
        value.funcall("restarts", ())?,
        value.funcall("window", ())?,
    )
}

pub(crate) fn intensity_value(ruby: &Ruby, intensity: Intensity) -> Result<Value, Error> {
    let window = nanos(ruby, intensity.window());
    let class = ruby.get_inner(classes(ruby)?.intensity);
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
            span: ruby.get_inner(classes.span),
            restart: ruby.get_inner(classes.restart),
        })
    }

    pub(crate) fn span(self, span: Span) -> Result<Value, Error> {
        self.span.funcall("new", (span.first(), span.last()))
    }

    /// `:ignore`, a `Restart`, `:escalate` or `:already_escalated`.
    pub(crate) fn decision(self, ruby: &Ruby, decision: Decision) -> Result<Value, Error> {
        match decision {
            Decision::Ignore => Ok(ruby.to_symbol("ignore").as_value()),
            Decision::Restart(span) => self.restart.funcall("new", (self.span(span)?,)),
            Decision::Escalate => Ok(ruby.to_symbol("escalate").as_value()),
            Decision::AlreadyEscalated => Ok(ruby.to_symbol("already_escalated").as_value()),
        }
    }
}

/// What `Intensity.new` stores: the validated restart count and window.
fn intensity_hook(ruby: &Ruby, restarts: Value, window: Value) -> Result<Value, Error> {
    let intensity = checked_intensity(ruby, restarts, window)?;
    Ok((intensity.restarts(), nanos(ruby, intensity.window())).into_value_with(ruby))
}

/// What `Span.new` stores: the validated ends.
fn span_hook(ruby: &Ruby, first: Value, last: Value) -> Result<(usize, usize), Error> {
    checked_span(ruby, first, last).map(|span| (span.first(), span.last()))
}

/// A policy symbol the core knows, or `TypeError`/`RangeError`.
fn policy_hook(ruby: &Ruby, value: Value) -> Result<Value, Error> {
    policy(ruby, value).map(|_| value)
}

/// A strategy symbol the core knows, or `TypeError`/`RangeError`.
fn strategy_hook(ruby: &Ruby, value: Value) -> Result<Value, Error> {
    strategy(ruby, value).map(|_| value)
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

/// A frozen array of the symbols naming a closed set.
fn names<T>(ruby: &Ruby, set: &[(&str, T)]) -> Result<RArray, Error> {
    frozen(ruby, set.iter().map(|&(name, _)| ruby.to_symbol(name)))
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = module(ruby)?;
    module.const_set("DEFAULT_RESTARTS", DEFAULT_RESTARTS)?;
    module.const_set("DEFAULT_WINDOW", nanos(ruby, DEFAULT_WINDOW))?;
    module.const_set("STRATEGIES", names(ruby, &STRATEGIES)?)?;
    module.const_set("POLICIES", names(ruby, &POLICIES)?)?;
    module.const_set("EXITS", names(ruby, &EXITS)?)?;
    resolve_classes(ruby, module)?;
    module.define_singleton_method("__intensity", function!(intensity_hook, 2))?;
    module.define_singleton_method("__span", function!(span_hook, 2))?;
    module.define_singleton_method("__policy", function!(policy_hook, 1))?;
    module.define_singleton_method("__strategy", function!(strategy_hook, 1))?;
    decider::define(ruby, module)
}
