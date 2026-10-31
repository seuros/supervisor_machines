//! Ruby bindings for the `supervisor-machines` decision machine.
//!
//! The core decides and nothing else: this crate converts Ruby symbols into
//! its enums, hands back a `Decision` as a symbol or a `Range`, and leaves
//! spawning, joining and killing to `SupervisorMachines::Supervisor`.
//!
//! Durations cross into the core as nanosecond integers. Seconds belong to the
//! Ruby surface, which converts them with [`NS_PER_SECOND`].

#![allow(clippy::needless_pass_by_value)]

mod decider;

use magnus::{Error, RArray, Ruby, Symbol, Value, prelude::*};
use supervisor_machines::{DEFAULT_RESTARTS, DEFAULT_WINDOW_NS, Decision, Exit, Policy, Strategy};

/// Nanoseconds in a second.
pub(crate) const NS_PER_SECOND: u32 = 1_000_000_000;

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

/// Resolves a symbol against one of the closed sets above.
fn member<T: Copy>(ruby: &Ruby, kind: &str, value: Value, set: &[(&str, T)]) -> Result<T, Error> {
    let name = Symbol::from_value(value).and_then(|symbol| symbol.name().ok());
    name.as_deref()
        .and_then(|name| set.iter().find(|(known, _)| *known == name))
        .map(|&(_, member)| member)
        .ok_or_else(|| {
            Error::new(
                ruby.exception_arg_error(),
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

pub(crate) fn strategy_name(ruby: &Ruby, strategy: Strategy) -> Symbol {
    let name = STRATEGIES
        .iter()
        .find(|&&(_, known)| known == strategy)
        .map_or("", |&(name, _)| name);
    ruby.to_symbol(name)
}

/// A [`Decision`] as Ruby reads it: `:ignore`, `:escalate`, or the inclusive
/// range of child indices to restart.
pub(crate) fn decision(ruby: &Ruby, decision: Decision) -> Result<Value, Error> {
    match decision {
        Decision::Ignore => Ok(ruby.to_symbol("ignore").as_value()),
        Decision::Escalate => Ok(ruby.to_symbol("escalate").as_value()),
        Decision::Restart(span) => ruby
            .range_new(span.first, span.last, false)
            .map(magnus::value::ReprValue::as_value),
    }
}

/// A frozen array of the symbols naming a closed set.
fn names<T>(ruby: &Ruby, set: &[(&str, T)]) -> Result<RArray, Error> {
    let array = ruby.ary_new_capa(set.len());
    for &(name, _) in set {
        array.push(ruby.to_symbol(name))?;
    }
    array.freeze();
    Ok(array)
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = ruby.define_module("SupervisorMachines")?;
    module.const_set("NS_PER_SECOND", NS_PER_SECOND)?;
    module.const_set("DEFAULT_RESTARTS", DEFAULT_RESTARTS)?;
    module.const_set("DEFAULT_WINDOW_NS", DEFAULT_WINDOW_NS)?;
    module.const_set("STRATEGIES", names(ruby, &STRATEGIES)?)?;
    module.const_set("POLICIES", names(ruby, &POLICIES)?)?;
    decider::define(ruby, module)
}
