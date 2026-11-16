//! Private native entry point.
use magnus::{Error, Ruby, function, prelude::*};
use matryona_machines::ruby::{Symbols, nanoseconds};
use supervisor_machines::{DEFAULT_RESTARTS, DEFAULT_WINDOW};

use crate::{
    Exits, Policies, Strategies, decider, intensity_hook, module, policy_hook, resolve_classes,
    restart_hook, span_hook, strategy_hook,
};

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let module = module(ruby)?;
    module.const_set("DEFAULT_RESTARTS", DEFAULT_RESTARTS)?;
    module.const_set("DEFAULT_WINDOW", nanoseconds(ruby, DEFAULT_WINDOW))?;
    module.const_set("STRATEGIES", Strategies::every(ruby))?;
    module.const_set("POLICIES", Policies::every(ruby))?;
    module.const_set("EXITS", Exits::every(ruby))?;
    resolve_classes(ruby, module)?;
    module.define_singleton_method("__intensity", function!(intensity_hook, 2))?;
    module.define_singleton_method("__span", function!(span_hook, 2))?;
    module.define_singleton_method("__restart", function!(restart_hook, 1))?;
    module.define_singleton_method("__policy", function!(policy_hook, 1))?;
    module.define_singleton_method("__strategy", function!(strategy_hook, 1))?;
    module.define_singleton_method(
        "__supervised_intensity",
        function!(decider::supervised_intensity_hook, 1),
    )?;
    decider::define(ruby, module)
}
