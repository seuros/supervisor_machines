# supervisor_machines

OTP-style supervision trees over Ruby threads.

A child died. Which children restart, and has that happened too often to keep
trying? Those answers come from the `no_std` [supervisor-machines][core] crate.
Everything the core refuses to touch — spawning, joining, killing, ordered
shutdown — is this gem.

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

## Strategies

`:one_for_one` restarts only what died. `:one_for_all` restarts every child,
for a set that only works together. `:rest_for_one` restarts the child and
everything started after it, for a chain where later children depend on earlier
ones.

One restart is recorded per failure handled, not per child brought back, so the
strategy does not change how fast the restart intensity is spent. That is OTP's
accounting.

## Policies

`:permanent` always comes back, even from a clean exit. `:transient` comes back
only from a bad one, because a clean exit means it finished. `:temporary` never
comes back, and never spends the restart budget.

```ruby
tree.supervise(:migration, policy: :temporary) { Thread.new { migrate } }
```

## Restart intensity

`restarts` within `window` seconds is all the thrashing a tree will do. Past
it, the tree stops its remaining children in reverse start order and raises
`SupervisorMachines::Escalated` — the supervisor's own supervisor now owns the
problem. That decision is sticky: `terminated?` stays true and every later
`stop` raises again.

## What this gem does not do

- The delay between a restart decision and the respawn comes from
  [chrono_machines][chrono]. Pass your own `backoff:` to change the curve.
- The shutdown budget comes from [deadline_machines][deadline]: `stop(grace:)`
  bounds the whole ordered teardown, and a child that outstays it is left
  behind rather than waited on forever.
- Restart intensity, strategy spans and policy decisions come from the core
  crate. None of it is reimplemented here.

## Shape

The core carries its child count in its type, so a tree is dispatched to one of
`SupervisorMachines::Decider::SUPPORTED_CHILDREN` monomorphised shapes: 1
through 16, and 32. Any other count raises `ArgumentError` from `#start`.

Threads are the only supervised unit.

## License

MIT.

[core]: ext/supervisor_machines_native/core/README.md
[chrono]: https://github.com/seuros/chrono_machines
[deadline]: https://github.com/seuros/deadline_machines
