//! `SupervisorMachines::Decider`: the core's decision machine, bound to Ruby.
//!
//! `Supervisor` carries its child count and its restart history in the type,
//! so a runtime child count is dispatched to one monomorphised shape per
//! supported count and kept behind [`Decide`].

use std::cell::RefCell;

use magnus::scan_args::{get_kwargs, scan_args};
use magnus::{Error, RArray, RHash, RModule, Ruby, Value, function, method, prelude::*};
use supervisor_machines::{Decision, Exit, Intensity, Policy, Strategy, Supervisor};

use crate::{Whole, decision, exit, policy, strategy, strategy_name};

/// Restart timestamps every shape keeps.
const HISTORY: usize = 64;

/// The core escalates one restart past the intensity, so the history has to
/// hold one more timestamp than the intensity tolerates.
#[expect(clippy::cast_possible_truncation)]
const MAX_RESTARTS: u32 = HISTORY as u32 - 1;

/// The child counts a [`Decider`] can be built for.
const SUPPORTED_CHILDREN: [usize; 17] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 32];

/// A [`Supervisor`] with its child count erased. A supervision tree is plain
/// data, so `Send` costs nothing and is what magnus asks of wrapped types.
trait Decide: Send {
    fn children(&self) -> usize;
    fn strategy(&self) -> Strategy;
    fn terminated(&self) -> bool;
    fn recent_restarts(&self, now_ns: u64) -> u32;
    fn stopped(&mut self, index: usize, policy: Policy, exit: Exit, now_ns: u64) -> Decision;
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

    fn terminated(&self) -> bool {
        self.0.terminated()
    }

    fn recent_restarts(&self, now_ns: u64) -> u32 {
        self.0.recent_restarts(now_ns)
    }

    fn stopped(&mut self, index: usize, policy: Policy, exit: Exit, now_ns: u64) -> Decision {
        self.0.stopped(index, policy, exit, now_ns)
    }
}

/// Each shape is built behind its own call frame. Inlined, a debug build sizes
/// one frame for every arm of the match in [`Wrapper::new`] at once, which
/// overflows Ruby's fiber stack.
#[inline(never)]
fn boxed<const CHILDREN: usize>(strategy: Strategy, intensity: Intensity) -> Box<dyn Decide> {
    Box::new(Shape(Supervisor::<CHILDREN, HISTORY>::new(
        strategy, intensity,
    )))
}

fn unsupported(ruby: &Ruby, children: usize) -> Error {
    Error::new(
        ruby.exception_arg_error(),
        format!(
            "{children} children is not a supported shape, expected one of {SUPPORTED_CHILDREN:?}"
        ),
    )
}

#[magnus::wrap(class = "SupervisorMachines::Decider", free_immediately, size)]
struct Wrapper(RefCell<Box<dyn Decide>>);

impl Wrapper {
    fn new(ruby: &Ruby, args: &[Value]) -> Result<Self, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (Whole(children), name, Whole(restarts), Whole(window_ns)) =
            get_kwargs::<_, (Whole<usize>, Value, Whole<u32>, Whole<u64>), (), ()>(
                args.keywords,
                &["children", "strategy", "restarts", "window_ns"],
                &[],
            )?
            .required;
        if restarts > MAX_RESTARTS {
            return Err(Error::new(
                ruby.exception_arg_error(),
                format!("{restarts} restarts cannot escalate, the most is {MAX_RESTARTS}"),
            ));
        }
        let strategy = strategy(ruby, name)?;
        let intensity = Intensity::new(restarts, window_ns);
        let tree = match children {
            1 => boxed::<1>(strategy, intensity),
            2 => boxed::<2>(strategy, intensity),
            3 => boxed::<3>(strategy, intensity),
            4 => boxed::<4>(strategy, intensity),
            5 => boxed::<5>(strategy, intensity),
            6 => boxed::<6>(strategy, intensity),
            7 => boxed::<7>(strategy, intensity),
            8 => boxed::<8>(strategy, intensity),
            9 => boxed::<9>(strategy, intensity),
            10 => boxed::<10>(strategy, intensity),
            11 => boxed::<11>(strategy, intensity),
            12 => boxed::<12>(strategy, intensity),
            13 => boxed::<13>(strategy, intensity),
            14 => boxed::<14>(strategy, intensity),
            15 => boxed::<15>(strategy, intensity),
            16 => boxed::<16>(strategy, intensity),
            32 => boxed::<32>(strategy, intensity),
            _ => return Err(unsupported(ruby, children)),
        };
        Ok(Self(RefCell::new(tree)))
    }

    /// Feeds one stopped child to the core and returns what it decided:
    /// `:ignore`, `:escalate`, or the inclusive range of children to restart.
    fn stopped(ruby: &Ruby, rb_self: &Self, args: &[Value]) -> Result<Value, Error> {
        let args = scan_args::<(), (), (), (), RHash, ()>(args)?;
        let (Whole(index), name, ending, Whole(now_ns)) =
            get_kwargs::<_, (Whole<usize>, Value, Value, Whole<u64>), (), ()>(
                args.keywords,
                &["index", "policy", "exit", "now_ns"],
                &[],
            )?
            .required;
        let (policy, ending) = (policy(ruby, name)?, exit(ruby, ending)?);
        let children = rb_self.0.borrow().children();
        if index >= children {
            return Err(Error::new(
                ruby.exception_arg_error(),
                format!("index {index} is outside the {children} children"),
            ));
        }
        let verdict = rb_self
            .0
            .borrow_mut()
            .stopped(index, policy, ending, now_ns);
        decision(ruby, verdict)
    }

    fn children(&self) -> usize {
        self.0.borrow().children()
    }

    fn strategy(ruby: &Ruby, rb_self: &Self) -> Value {
        strategy_name(ruby, rb_self.0.borrow().strategy()).as_value()
    }

    fn terminated(&self) -> bool {
        self.0.borrow().terminated()
    }

    fn recent_restarts(&self, Whole(now_ns): Whole<u64>) -> u32 {
        self.0.borrow().recent_restarts(now_ns)
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
    class.const_set("HISTORY", HISTORY)?;
    class.const_set("MAX_RESTARTS", MAX_RESTARTS)?;
    class.const_set("SUPPORTED_CHILDREN", supported_children(ruby)?)?;
    class.define_singleton_method("new", function!(Wrapper::new, -1))?;
    class.define_method("stopped", method!(Wrapper::stopped, -1))?;
    class.define_method("children", method!(Wrapper::children, 0))?;
    class.define_method("strategy", method!(Wrapper::strategy, 0))?;
    class.define_method("terminated?", method!(Wrapper::terminated, 0))?;
    class.define_method("recent_restarts", method!(Wrapper::recent_restarts, 1))?;
    class.define_method("inspect", method!(Wrapper::inspect, 0))?;
    Ok(())
}

fn supported_children(ruby: &Ruby) -> Result<RArray, Error> {
    let array = ruby.ary_new_capa(SUPPORTED_CHILDREN.len());
    for children in SUPPORTED_CHILDREN {
        array.push(children)?;
    }
    array.freeze();
    Ok(array)
}
