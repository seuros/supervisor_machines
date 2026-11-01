# SupervisorMachines 🔁

> **OTP-style supervision trees over Ruby threads.** The command vessel that
> notices a station has gone quiet and decides who returns to duty.

A child died. Which children restart, and has that happened too often to keep
trying? Those answers come from the `no_std` [supervisor-machines][core] crate.
Everything the core refuses to touch, spawning, joining, killing, ordered
shutdown, is this gem. **AMS Armstrong** is the bridge around that decision.

---

## 🚀 Launch Sequence

```bash
# Add to your ship's Gemfile
gem 'supervisor_machines'
```

Requires **Ruby 4.0+** and a Rust toolchain, or a platform gem: the core is a
native extension, not an option.

---

## 🎯 Mission Brief

```ruby
SM = SupervisorMachines

tree = SM::Supervisor.new(strategy: :one_for_all, restarts: 3, window: 60.0)
tree.supervise(:cache)  { Thread.new { cache.run } }
tree.supervise(:warmup, policy: :transient) { Thread.new { warm_the_cache } }
tree.start

tree.children    # => [:cache, :warmup]
tree.restarts    # => 0
tree.terminated? # => false

tree.stop(grace: 5.0)
```

A starter block returns the `Thread` that runs the child, and is called again
for every restart. A child that stops is noticed, handed to the core, and
whatever the core decides is carried out.

---

## 🔄 Lifecycle

A tree registers children, runs them, and ends either stopped or failed. Both
endings are final: `start` raises `SupervisorMachines::Error` for a tree that
is already running, has been stopped, or gave up. There is one generation of
children per tree, and starting a second one is a bug, not a refresh.
`supervise` is refused the same way once `start` has fixed the shape of the
tree: a tree past registering cannot take on a child.

A `start` that cannot finish, because a starter raises, takes the children it
already brought up back down in reverse start order, leaves the tree failed,
and raises what the starter raised. Nothing is left running and nothing is left
unsupervised. `stop` then raises `Escalated`, like any other tree that gave up,
every time it is called, and a tree that was never started has nothing to take
down.

---

## 🎛️ Strategies

`:one_for_one` restarts only what died. `:one_for_all` restarts every child,
for a set that only works together. `:rest_for_one` restarts the child and
everything started after it, for a chain where later children depend on earlier
ones.

One restart is recorded per failure handled, not per child brought back, so the
strategy does not change how fast the restart intensity is spent. That is OTP's
accounting.

---

## 📋 Standing Orders

A child's policy is its standing order. `:permanent` always comes back, even
from a clean exit. `:transient` comes back
only from a bad one, because a clean exit means it finished. `:temporary` never
comes back, and never spends the restart budget.

```ruby
tree.supervise(:migration, policy: :temporary) { Thread.new { migrate } }
```

---

## 🔥 Restart Intensity

`restarts` within `window` seconds is all the thrashing a tree will do. Past
it, the tree stops its remaining children in reverse start order and raises
`SupervisorMachines::Escalated`, and the supervisor's own supervisor now owns
the problem. That decision is sticky: `terminated?` stays true and every later
`stop` raises again.

---

## 🚫 Rules of Engagement

- The delay between a restart decision and the respawn comes from
  [chrono_machines][chrono]. Pass your own `backoff:` to change the curve.
- The shutdown budget comes from [deadline_machines][deadline]: `stop(grace:)`
  bounds the whole ordered teardown, and a child that outstays it is left
  behind rather than waited on forever.
- Restart intensity, strategy spans and policy decisions come from the core
  crate. None of it is reimplemented here.

---

## 📐 Shape

The core carries its child count in its type, so a tree is dispatched to one of
`SupervisorMachines::Decider::SUPPORTED_CHILDREN` monomorphised shapes: 1
through 16, and 32. Any other count raises `ArgumentError` from `#start`.

Threads are the only supervised unit.

---

## 📡 Credits

**AMS Armstrong**, NCC-3186, *Armstrong*-class command vessel.

Named for Joe Armstrong's OTP supervision trees, which settled that the way to
keep a system up is to let a part of it die and have something above it decide
what comes back.

---

## License

MIT.

[core]: ext/supervisor_machines_native/core/README.md
[chrono]: https://github.com/seuros/chrono_machines
[deadline]: https://github.com/seuros/deadline_machines
