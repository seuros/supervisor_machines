# frozen_string_literal: true

module SupervisorMachines
  # A supervision tree over threads.
  #
  # Which children a failure takes down, and whether restarting is still worth
  # trying, belongs entirely to {Decider}. This class owns the lifecycle:
  # spawning children, noticing when one stops, waiting out the backoff
  # +chrono_machines+ hands it, and taking the tree down in reverse start order
  # inside a +deadline_machines+ budget.
  #
  # A tree is +registering+ until {#start} fixes its shape, +running+ while it
  # watches its children, and ends either +stopped+ or +failed+. Both endings
  # are final: {#start} refuses a tree that is not registering, and a +failed+
  # tree answers every {#stop} with {Escalated}.
  #
  # Every instant and span it takes or reports is Integer nanoseconds, as
  # deadline_machines takes them; only Thread#join gets Float seconds.
  #
  #   minute = 60 * 1_000_000_000
  #   tree = SupervisorMachines::Supervisor.new(:one_for_all, SupervisorMachines::Intensity.new(3, minute))
  #   tree.supervise(:cache) { Thread.new { cache.run } }
  #   tree.supervise(:warmup, policy: :transient) { Thread.new { warm } }
  #   tree.start
  #   tree.stop(5 * 1_000_000_000)
  class Supervisor
    # A registered child and whichever thread currently runs it. +epoch+ rises
    # every time the child is taken down, so a thread the tree has already
    # replaced neither reports its exit nor takes the child's place when its
    # starter returns late. +done+ marks a child the tree has dropped, which no
    # group restart brings back. A child is replaced, never mutated, and only
    # under the tree's lock.
    Child = Data.define(:name, :policy, :starter, :thread, :epoch, :done) do
      def initialize(name:, policy:, starter:, thread:, epoch:, done:)
        policy = SupervisorMachines.__send__(:__policy, policy)
        super
      end
    end
    Child.singleton_class.undef_method(:allocate)
    Child.define_singleton_method(:inherited) { |_| raise TypeError, "#{Child} cannot be subclassed" }
    private_constant :Child

    # Converts a budget in nanoseconds into the seconds Thread#join takes.
    NS_PER_SECOND = 1_000_000_000
    private_constant :NS_PER_SECOND

    # The respawn delay when none is given: 100 ms doubling to 10 s, full
    # jitter.
    BACKOFF = ChronoMachines::Exponential.new(NS_PER_SECOND / 10, 2.0, 10 * NS_PER_SECOND, 1.0)

    # Jitter draws are uniform over 0...DRAWS.
    DRAWS = 1 << 64
    private_constant :DRAWS

    # Wakes the monitor when the tree is being taken down.
    STOP = Object.new
    private_constant :STOP

    attr_reader :strategy, :intensity

    # +strategy+ is one of STRATEGIES and +intensity+ an {Intensity}, as the
    # core's +Supervisor::new+ takes them; past the intensity the tree
    # escalates. TypeError for the wrong types, RangeError for an unknown
    # strategy or more restarts than {Decider::MAX_RESTARTS}. +backoff+ is
    # asked for the delay between a restart decision and the respawn:
    # anything answering +delay(attempt, draw)+ with Integer nanoseconds, a
    # +chrono_machines+ shape ({BACKOFF} unless told otherwise). It has no
    # attempt budget of its own: the core decides when the tree escalates.
    #
    # +backoff:+ and {#supervise}'s +policy: :permanent+ are keyword defaults
    # the core has no counterpart for. They stay because this class is a
    # runtime on top of the core, not a mirror of it; {Decider} is the mirror.
    def initialize(strategy, intensity, backoff: nil)
      strategy = SupervisorMachines.__send__(:__strategy, strategy)
      # Read natively, never through `instance_of?` or the readers, and
      # rebuilt through the core: `Marshal.load` skips `initialize`.
      intensity = SupervisorMachines.__send__(:__supervised_intensity, intensity)

      @strategy = strategy
      @intensity = intensity
      backoff ||= BACKOFF
      raise TypeError, "backoff must answer delay, got #{backoff.inspect}" unless backoff.respond_to?(:delay)

      @backoff = backoff
      @slots = []
      @events = Thread::Queue.new
      @mutex = Thread::Mutex.new
      @halt = Thread::ConditionVariable.new
      @state = :registering
      @failed = nil
    end

    # Registers a child under +name+. The block is its starter: it returns the
    # +Thread+ that runs the child, and is called again for every restart.
    # TypeError for a +policy+ that is not a Symbol, RangeError for one outside
    # POLICIES.
    def supervise(name, policy: :permanent, &starter)
      child = Child.new(name, policy, starter, nil, 0, false)
      @mutex.synchronize do
        raise Error, "a #{@state} supervisor cannot take on a child" unless @state == :registering

        @slots << child
      end
      self
    end

    # Fixes the shape of the tree, starts every child in registration order and
    # begins watching them. A starter that will not run takes the children
    # already up back down, leaves the tree failed, and raises what the starter
    # raised.
    #
    # Raises {Error} for a tree that is past registering, RangeError for a
    # child count the core has no shape for.
    def start
      @mutex.synchronize do
        raise Error, "a #{@state} supervisor cannot be started" unless @state == :registering

        @decider = Decider.new(@slots.size, @strategy, @intensity)
        @state = :running
        @monitor = Thread.new { monitor }
      end
      failure = launch
      return self unless failure

      rollback
      raise failure
    end

    # Child names, in start order.
    def children
      @mutex.synchronize { @slots.map(&:name) }
    end

    # Restarts still inside the intensity window.
    def restarts
      now = monotonic_ns
      @mutex.synchronize { @decider ? @decider.recent_restarts(now) : 0 }
    end

    # True once the tree has escalated, and true from then on.
    def terminated?
      @mutex.synchronize { @state == :failed }
    end

    # Takes every running child down in reverse start order, giving the whole
    # shutdown +grace+ nanoseconds, or all the time it needs when +grace+ is
    # nil. A tree still registering has nothing to take down and stays
    # registering. TypeError for a non-Integer +grace+, RangeError for a
    # negative one, both before anything is touched.
    #
    # Raises {Escalated} for a tree that gave up, every time it is called.
    def stop(grace = nil)
      budget = grace.nil? ? nil : grace_ns(grace)
      entered = @mutex.synchronize do
        state = @state
        @state = :stopped if state == :running
        @halt.broadcast
        state
      end
      return self if entered == :registering

      @events.push(STOP) if entered == :running
      failures = nil
      DeadlineMachines.with_deadline(budget) do |deadline|
        failures = shutdown
        @monitor.kill unless @monitor.join(join_seconds(deadline.remaining))
      end
      raise Escalated, escalation if terminated?
      raise failures.first unless failures.empty?

      self
    end

    # Nanoseconds a supervisor taking itself down waits for a killed child,
    # since neither the monitor thread nor a failing {#start} has a caller
    # budget to read.
    ESCALATION_GRACE = 5 * NS_PER_SECOND

    private

    # Brings every child up in registration order, returning what a starter
    # raised, or nil once the whole tree is up.
    def launch
      @slots.each_index { |index| spawn(index) }
      nil
    rescue Exception => e
      @mutex.synchronize do
        @state = :failed
        @failed = unstarted(e)
      end
      e
    end

    # Takes down whatever a start brought up before it failed, the children and
    # the monitor watching them, inside one budget.
    def rollback
      DeadlineMachines.with_deadline(ESCALATION_GRACE) do |deadline|
        shutdown
        reap(@monitor, deadline.remaining)
      end
    end

    def running?
      @state == :running
    end

    def monitor
      Thread.current.report_on_exception = false
      loop do
        event = @events.pop
        break if event.equal?(STOP)

        handle(*event)
      end
    end

    # One child stopped. The core says what that means; this carries it out.
    def handle(index, epoch, ending)
      now = monotonic_ns
      decision = @mutex.synchronize do
        child = @slots[index]
        next unless running? && child.epoch == epoch

        @slots[index] = child.with(thread: nil, done: child.done || child.policy == :temporary)
        @decider.stopped(index, child.policy, ending, now)
      end

      case decision
      when Restart then restart(decision.span)
      when :escalate then escalate
      # Nothing to carry out: a child left stopped, a tree that already gave
      # up, an index the tree does not own (it only asks about its own), or
      # an event from a generation the tree has replaced.
      when :ignore, :already_escalated, :unknown_child, nil then nil
      end
    end

    # Retires the whole span in reverse start order and waits for it to go,
    # waits out the backoff without holding the tree, then brings the span back
    # in index order.
    def restart(span)
      retiring = @mutex.synchronize { span.last.downto(span.first).filter_map { |index| retire(index) } }
      DeadlineMachines.with_deadline(ESCALATION_GRACE) do |deadline|
        retiring.each { |thread| reap(thread, deadline.remaining) }
      end
      return escalate(abandoned) if retiring.any?(&:alive?)

      failure = respawn(span)
      escalate(failure) if failure
    end

    # The reason a respawn could not happen, or nil once the span is running.
    def respawn(span)
      delay = @backoff.delay(restarts.clamp(1, 255), Random.rand(DRAWS))
      @mutex.synchronize { @halt.wait(@mutex, delay.fdiv(NS_PER_SECOND)) if delay.positive? && running? }
      span.first.upto(span.last) { |index| spawn(index) }
      nil
    rescue Exception => e
      unstarted(e)
    end

    def escalate(reason = escalation)
      @mutex.synchronize do
        @state = :failed
        @failed = reason
      end
      shutdown(ESCALATION_GRACE)
      raise Escalated, reason
    end

    # Reverse start order, each child killed and then waited for inside one
    # budget of +grace+ nanoseconds (nil: no limit) shared by the whole tree. Touches no tree state, so a shutdown
    # under way in the monitor never holds up the one in {#stop}. Returns what
    # the children raised on their way out, having stopped all of them.
    def shutdown(grace = DeadlineMachines.remaining)
      failures = []
      DeadlineMachines.with_deadline(grace) do |deadline|
        @slots.reverse_each do |child|
          thread = child.thread
          next unless thread

          failures << reap(thread, deadline.remaining)
        end
      end
      failures.compact
    end

    # Kills the thread and waits out the rest of the budget, +remaining+
    # nanoseconds or nil for no limit, for it, reporting whatever it raised on
    # the way out. A thread that outlives the budget is still alive afterwards.
    def reap(thread, remaining)
      thread.kill
      thread.join(join_seconds(remaining))
      nil
    rescue Exception => e
      e
    end

    # Calls the child's starter outside the lock, under the generation the tree
    # is in when the call goes out. A child the tree has dropped, and every
    # child of a tree that is no longer running, is left alone.
    def spawn(index)
      generation, starter = @mutex.synchronize do
        child = @slots[index]
        [child.epoch, child.starter] if running? && !child.done
      end
      install(index, generation, starter.call) if generation
    end

    # Hands the started thread to the child, unless the tree has moved past the
    # generation that asked for it, in which case the thread is killed instead
    # of left running with nothing watching it.
    def install(index, generation, thread)
      installed = @mutex.synchronize do
        child = @slots[index]
        next false unless running? && child.epoch == generation

        @slots[index] = child.with(thread: thread)
        watch(index, generation, thread)
        true
      end
      reap(thread, ESCALATION_GRACE) unless installed
    end

    # Invalidates the watcher following the child at +index+ and hands its
    # thread back to be reaped outside the lock.
    def retire(index)
      child = @slots[index]
      @slots[index] = child.with(epoch: child.epoch + 1, thread: nil)
      child.thread
    end

    # One thread per running child, blocked on it, reporting how it ended.
    def watch(index, epoch, thread)
      Thread.new do
        thread.join
        @events.push([index, epoch, :normal])
      rescue Exception
        @events.push([index, epoch, :abnormal])
      end
    end

    def escalation
      @failed || "#{@strategy} supervisor escalated: more than #{@intensity.restarts} restarts in " \
                 "#{@intensity.window} ns"
    end

    def abandoned
      "#{@strategy} supervisor escalated: a retired child would not stop within #{ESCALATION_GRACE} ns"
    end

    def unstarted(error)
      "#{@strategy} supervisor escalated: #{error.message}"
    end

    def monotonic_ns
      DeadlineMachines.clock.now_ns
    end

    # A grace checked before anything is touched: Integer nanoseconds.
    def grace_ns(nanos)
      raise TypeError, "a grace is Integer nanoseconds, got #{nanos.inspect}" unless nanos.is_a?(Integer)
      raise RangeError, "a grace cannot be negative: #{nanos}" if nanos.negative?

      nanos
    end

    # A budget in nanoseconds, or nil for none, as Thread#join's timeout.
    def join_seconds(nanos)
      nanos&.fdiv(NS_PER_SECOND)
    end
  end
end
