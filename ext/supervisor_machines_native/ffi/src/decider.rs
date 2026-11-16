//! `SupervisorMachines::Decider`: the core's decision machine, bound to Ruby.
//!
//! `Supervisor` carries its child count and its restart history in the type,
//! so a runtime child count is dispatched to one monomorphised shape per
//! supported count and kept behind [`Decide`].
//!
//! Every mutating method checks frozen first, then converts and checks all of
//! its arguments and looks up every class it answers with, before it calls
//! the core; afterwards it
//! only builds values from what the core returned. `stopped` answers with a
//! `Restart` built by Ruby, so it runs in `matryona_machines`'s
//! clone-then-keep: decided on a copy, kept only once that answer exists and
//! the decider is neither frozen nor changed. A raise while building it
//! changes nothing, freezing the decider meanwhile raises `FrozenError` and a
//! re-entrant change `RuntimeError`, in both cases with the outer call not
//! applied. No `RefCell` borrow is held while Ruby code runs.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

use magnus::typed_data::Obj;
use magnus::{Error, Integer, RArray, RModule, Ruby, Value, function, method, prelude::*};
use matryona_machines::ruby::{
    Symbols, class_name, duration, exact_array_with, integer, keep, nanoseconds,
};
use supervisor_machines::{
    Decision, Exit, Intensity, Policy, Recording, Refusal, Span, Strategy, Supervisor,
};

use crate::{
    DecisionClasses, Exits, Policies, Recordings, Refusals, Strategies, frozen, intensity_from,
    intensity_value,
};

/// Restart instants every shape keeps.
const HISTORY: usize = 64;

/// The core escalates one restart past the intensity, so the history has to
/// hold one more instant than the intensity tolerates.
#[expect(clippy::cast_possible_truncation)]
const MAX_RESTARTS: u32 = HISTORY as u32 - 1;

/// The child counts a [`Decider`] can be built for.
const SUPPORTED_CHILDREN: [usize; 17] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 32];

/// A [`Supervisor`] with its child count erased. A supervision tree is plain
/// data, so `Send` costs nothing and is what magnus asks of wrapped types.
trait Decide: Send {
    fn children(&self) -> usize;
    fn strategy(&self) -> Strategy;
    fn intensity(&self) -> Intensity;
    fn terminated(&self) -> bool;
    fn history(&self) -> Vec<Duration>;
    fn span(&self, index: usize) -> Option<Span>;
    fn recent_restarts(&self, now: Duration) -> u32;
    fn restart_delay(&self, base: Duration, ceiling: Duration, now: Duration) -> Duration;
    fn stopped(
        &mut self,
        index: usize,
        policy: Policy,
        exit: Exit,
        now: Duration,
    ) -> Result<Decision, Refusal>;
    fn record_external_restart(&mut self, now: Duration) -> Recording;
    fn absorb(&mut self, history: &[Duration], now: Duration) -> usize;
    fn duplicate(&self) -> Box<dyn Decide>;

    /// Whether `other` holds the same state. The shape, strategy and
    /// intensity never change after construction, so the history and the
    /// escalation are all there is to compare.
    fn same(&self, other: &dyn Decide) -> bool {
        self.terminated() == other.terminated() && self.history() == other.history()
    }
}

/// One monomorphised supervision tree. The newtype keeps the trait's method
/// names from shadowing the core's inherent ones.
struct Shape<const CHILDREN: usize>(Supervisor<CHILDREN, HISTORY>);

impl<const CHILDREN: usize> Decide for Shape<CHILDREN> {
    fn children(&self) -> usize {
        CHILDREN
    }

    fn strategy(&self) -> Strategy {
        self.0.strategy()
    }

    fn intensity(&self) -> Intensity {
        self.0.intensity()
    }

    fn terminated(&self) -> bool {
        self.0.terminated()
    }

    fn history(&self) -> Vec<Duration> {
        self.0.history().collect()
    }

    fn span(&self, index: usize) -> Option<Span> {
        self.0.span(index)
    }

    fn recent_restarts(&self, now: Duration) -> u32 {
        self.0.recent_restarts(now)
    }

    fn restart_delay(&self, base: Duration, ceiling: Duration, now: Duration) -> Duration {
        self.0.restart_delay(base, ceiling, now)
    }

    fn stopped(
        &mut self,
        index: usize,
        policy: Policy,
        exit: Exit,
        now: Duration,
    ) -> Result<Decision, Refusal> {
        self.0.stopped(index, policy, exit, now)
    }

    fn record_external_restart(&mut self, now: Duration) -> Recording {
        self.0.record_external_restart(now)
    }

    fn absorb(&mut self, history: &[Duration], now: Duration) -> usize {
        self.0.absorb(history, now)
    }

    fn duplicate(&self) -> Box<dyn Decide> {
        Box::new(Self(self.0))
    }
}

/// A decider's tree, copied by [`Decide::duplicate`] and compared by
/// [`Decide::same`], so clone-then-keep can hold it.
struct Tree(Box<dyn Decide>);

impl Clone for Tree {
    fn clone(&self) -> Self {
        Self(self.0.duplicate())
    }
}

impl PartialEq for Tree {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(&*other.0)
    }
}

impl Deref for Tree {
    type Target = dyn Decide;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl DerefMut for Tree {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.0
    }
}

/// A saved history and the instant it is restored at.
type Saved<'a> = Option<(&'a [Duration], Duration)>;

/// Each shape is built behind its own call frame. Inlined, a debug build sizes
/// one frame for every arm of the match in [`Wrapper::build`] at once, which
/// overflows Ruby's fiber stack.
#[inline(never)]
fn boxed<const CHILDREN: usize>(
    strategy: Strategy,
    intensity: Intensity,
    saved: Saved<'_>,
) -> Option<Box<dyn Decide>> {
    let tree = match saved {
        None => Supervisor::<CHILDREN, HISTORY>::new(strategy, intensity),
        Some((history, now)) => {
            Supervisor::restore(strategy, intensity, history.iter().copied(), now)
        }
    }?;
    Some(Box::new(Shape(tree)))
}

fn shape(
    children: usize,
    strategy: Strategy,
    intensity: Intensity,
    saved: Saved<'_>,
) -> Option<Box<dyn Decide>> {
    match children {
        1 => boxed::<1>(strategy, intensity, saved),
        2 => boxed::<2>(strategy, intensity, saved),
        3 => boxed::<3>(strategy, intensity, saved),
        4 => boxed::<4>(strategy, intensity, saved),
        5 => boxed::<5>(strategy, intensity, saved),
        6 => boxed::<6>(strategy, intensity, saved),
        7 => boxed::<7>(strategy, intensity, saved),
        8 => boxed::<8>(strategy, intensity, saved),
        9 => boxed::<9>(strategy, intensity, saved),
        10 => boxed::<10>(strategy, intensity, saved),
        11 => boxed::<11>(strategy, intensity, saved),
        12 => boxed::<12>(strategy, intensity, saved),
        13 => boxed::<13>(strategy, intensity, saved),
        14 => boxed::<14>(strategy, intensity, saved),
        15 => boxed::<15>(strategy, intensity, saved),
        16 => boxed::<16>(strategy, intensity, saved),
        32 => boxed::<32>(strategy, intensity, saved),
        _ => None,
    }
}

/// Integer nanosecond instants, all converted before the core sees any. Only
/// an `Array` is read, never a subclass that could answer for its elements.
fn instants(ruby: &Ruby, value: Value) -> Result<Vec<Duration>, Error> {
    let array = exact_array_with(ruby, value, || {
        format!(
            "history must be an Array of Integers, not {}",
            class_name(value)
        )
    })?;
    array
        .into_iter()
        .map(|item| duration(item, "history"))
        .collect()
}

/// The `RangeError` for an intensity the history cannot enforce.
fn cannot_escalate(ruby: &Ruby, intensity: Intensity) -> Error {
    Error::new(
        ruby.exception_range_error(),
        format!(
            "{} restarts cannot escalate, the most is {MAX_RESTARTS}",
            intensity.restarts()
        ),
    )
}

/// What `Supervisor.new` runs under: a `SupervisorMachines::Intensity` read
/// natively (exact class, fields from the struct), rebuilt through the core,
/// and refused past [`MAX_RESTARTS`] with the errors `Decider.new` gives, so
/// a tree that cannot escalate fails before it takes on a child.
pub fn supervised_intensity_hook(ruby: &Ruby, value: Value) -> Result<Value, Error> {
    let intensity = intensity_from(ruby, value)?;
    if intensity.restarts() > MAX_RESTARTS {
        return Err(cannot_escalate(ruby, intensity));
    }
    intensity_value(ruby, intensity)
}

#[magnus::wrap(class = "SupervisorMachines::Decider", free_immediately, size)]
struct Wrapper(RefCell<Tree>);

impl Wrapper {
    /// `RangeError` for a child count with no shape, and for an intensity the
    /// history cannot enforce (the core's `Supervisor::new` answering `None`).
    fn build(
        ruby: &Ruby,
        children: Value,
        name: Value,
        intensity: Value,
        saved: Saved<'_>,
    ) -> Result<Self, Error> {
        let children: usize = integer(children, "children")?;
        let strategy = Strategies::parse(ruby, name)?;
        let intensity = intensity_from(ruby, intensity)?;
        if !SUPPORTED_CHILDREN.contains(&children) {
            return Err(Error::new(
                ruby.exception_range_error(),
                format!(
                    "{children} children is not a supported shape, expected one of \
                     {SUPPORTED_CHILDREN:?}"
                ),
            ));
        }
        let tree = shape(children, strategy, intensity, saved)
            .ok_or_else(|| cannot_escalate(ruby, intensity))?;
        Ok(Self(RefCell::new(Tree(tree))))
    }

    fn new(ruby: &Ruby, children: Value, name: Value, intensity: Value) -> Result<Self, Error> {
        Self::build(ruby, children, name, intensity, None)
    }

    fn restore(
        ruby: &Ruby,
        children: Value,
        name: Value,
        intensity: Value,
        history: Value,
        now: Value,
    ) -> Result<Self, Error> {
        let (history, now) = (instants(ruby, history)?, duration(now, "now")?);
        Self::build(ruby, children, name, intensity, Some((&history, now)))
    }

    /// `:ignore`, a `Restart`, `:escalate`, `:already_escalated`, or the
    /// refusal `:unknown_child`. Decided on a copy, kept only once the answer
    /// is built: `RuntimeError` when the decider changed meanwhile.
    fn stopped(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        index: Value,
        name: Value,
        ending: Value,
        now: Value,
    ) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let index: usize = integer(index, "index")?;
        let (policy, ending) = (Policies::parse(ruby, name)?, Exits::parse(ruby, ending)?);
        let now = duration(now, "now")?;
        let classes = DecisionClasses::fetch(ruby)?;
        keep(
            ruby,
            rb_self,
            &rb_self.0,
            |tree| tree.stopped(index, policy, ending, now),
            |decided| match decided {
                Ok(decision) => classes.decision(ruby, decision),
                Err(refusal) => Ok(Refusals::symbol(ruby, refusal).as_value()),
            },
        )
    }

    /// `:recorded` or `:outdated`. Never refused.
    fn record_external_restart(
        ruby: &Ruby,
        rb_self: Obj<Self>,
        now: Value,
    ) -> Result<Value, Error> {
        rb_self.check_frozen()?;
        let now = duration(now, "now")?;
        let recording = rb_self.0.borrow_mut().record_external_restart(now);
        Ok(Recordings::symbol(ruby, recording).as_value())
    }

    /// How many restarts were added, keeping the larger count of each
    /// instant across both histories. Never refused.
    fn absorb(ruby: &Ruby, rb_self: Obj<Self>, history: Value, now: Value) -> Result<usize, Error> {
        rb_self.check_frozen()?;
        let (history, now) = (instants(ruby, history)?, duration(now, "now")?);
        Ok(rb_self.0.borrow_mut().absorb(&history, now))
    }

    fn history(ruby: &Ruby, rb_self: &Self) -> Result<RArray, Error> {
        let held = rb_self.0.borrow().history();
        frozen(ruby, held.into_iter().map(|at| nanoseconds(ruby, at)))
    }

    fn span(ruby: &Ruby, rb_self: &Self, index: Value) -> Result<Value, Error> {
        let index: usize = integer(index, "index")?;
        let classes = DecisionClasses::fetch(ruby)?;
        let span = rb_self.0.borrow().span(index);
        span.map_or_else(|| Ok(ruby.qnil().as_value()), |span| classes.span(span))
    }

    fn children(&self) -> usize {
        self.0.borrow().children()
    }

    fn strategy(ruby: &Ruby, rb_self: &Self) -> Value {
        let strategy = rb_self.0.borrow().strategy();
        Strategies::symbol(ruby, strategy).as_value()
    }

    fn intensity(ruby: &Ruby, rb_self: &Self) -> Result<Value, Error> {
        // Copied out first: `Intensity.new` is Ruby code and may re-enter.
        let intensity = rb_self.0.borrow().intensity();
        intensity_value(ruby, intensity)
    }

    fn terminated(&self) -> bool {
        self.0.borrow().terminated()
    }

    fn recent_restarts(&self, now: Value) -> Result<u32, Error> {
        let now = duration(now, "now")?;
        Ok(self.0.borrow().recent_restarts(now))
    }

    fn restart_delay(
        ruby: &Ruby,
        rb_self: &Self,
        base: Value,
        ceiling: Value,
        now: Value,
    ) -> Result<Integer, Error> {
        let (base, ceiling) = (duration(base, "base")?, duration(ceiling, "ceiling")?);
        let now = duration(now, "now")?;
        Ok(nanoseconds(
            ruby,
            rb_self.0.borrow().restart_delay(base, ceiling, now),
        ))
    }

    fn inspect(ruby: &Ruby, rb_self: &Self) -> String {
        format!(
            "#<SupervisorMachines::Decider strategy={} children={} terminated={}>",
            Self::strategy(ruby, rb_self).inspect(),
            rb_self.children(),
            rb_self.terminated()
        )
    }
}

pub fn define(ruby: &Ruby, module: RModule) -> Result<(), Error> {
    let class = module.define_class("Decider", ruby.class_object())?;
    class.undef_default_alloc_func();
    class.const_set("HISTORY", HISTORY)?;
    class.const_set("MAX_RESTARTS", MAX_RESTARTS)?;
    class.const_set(
        "SUPPORTED_CHILDREN",
        frozen(ruby, SUPPORTED_CHILDREN.into_iter())?,
    )?;
    class.define_singleton_method("new", function!(Wrapper::new, 3))?;
    class.define_singleton_method("restore", function!(Wrapper::restore, 5))?;
    class.define_method("stopped", method!(Wrapper::stopped, 4))?;
    class.define_method(
        "record_external_restart",
        method!(Wrapper::record_external_restart, 1),
    )?;
    class.define_method("absorb", method!(Wrapper::absorb, 2))?;
    class.define_method("history", method!(Wrapper::history, 0))?;
    class.define_method("span", method!(Wrapper::span, 1))?;
    class.define_method("children", method!(Wrapper::children, 0))?;
    class.define_method("strategy", method!(Wrapper::strategy, 0))?;
    class.define_method("intensity", method!(Wrapper::intensity, 0))?;
    class.define_method("terminated?", method!(Wrapper::terminated, 0))?;
    class.define_method("recent_restarts", method!(Wrapper::recent_restarts, 1))?;
    class.define_method("restart_delay", method!(Wrapper::restart_delay, 3))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}
