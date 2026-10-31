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
  #   tree = SupervisorMachines::Supervisor.new(strategy: :one_for_all, restarts: 3, window: 60.0)
  #   tree.supervise(:cache) { Thread.new { cache.run } }
  #   tree.supervise(:warmup, policy: :transient) { Thread.new { warm } }
  #   tree.start
  #   tree.stop(grace: 5.0)
  class Supervisor
    # A registered child and whichever thread currently runs it. +epoch+ rises
    # every time the child is taken down, so an exit reported by a thread the
    # tree has already replaced is discarded. +done+ marks a child the tree has
    # dropped, which no group restart brings back.
    Child = Struct.new(:name, :policy, :starter, :thread, :epoch, :done)

    # Wakes the monitor when the tree is being taken down.
    STOP = Object.new
    private_constant :STOP

    attr_reader :strategy

    # +restarts+ within +window+ seconds is the restart intensity; past it the
    # tree escalates. +backoff+ is asked for the delay between a restart
    # decision and the respawn; its attempt budget has to be one wider than the
    # intensity, because the core escalates one restart after the last one it
    # tolerates.
    def initialize(strategy: :one_for_one, restarts: DEFAULT_RESTARTS,
                   window: DEFAULT_WINDOW_NS / NS_PER_SECOND.to_f, backoff: nil)
      raise ArgumentError, "unknown strategy: #{strategy.inspect}" unless STRATEGIES.include?(strategy)
      raise ArgumentError, "at most #{Decider::MAX_RESTARTS} restarts" if restarts > Decider::MAX_RESTARTS

      @strategy = strategy
      @restarts = restarts
      @window = window
      @backoff = backoff || ChronoMachines::Executor.new(max_attempts: restarts + 1)
      @slots = []
      @events = Thread::Queue.new
      @mutex = Thread::Mutex.new
      @halt = Thread::ConditionVariable.new
      @state = :registering
      @failed = nil
    end

    # Registers a child under +name+. The block is its starter: it returns the
    # +Thread+ that runs the child, and is called again for every restart.
    def supervise(name, policy: :permanent, &starter)
      raise ArgumentError, "unknown policy: #{policy.inspect}" unless POLICIES.include?(policy)

      @mutex.synchronize { @slots << Child.new(name, policy, starter, nil, 0, false) }
      self
    end

    # Fixes the shape of the tree, starts every child in registration order and
    # begins watching them. A starter that will not run takes the children
    # already up back down, leaves the tree failed, and raises what the starter
    # raised.
    #
    # Raises {Error} for a tree that is past registering.
    def start
      @mutex.synchronize do
        raise Error, "a #{@state} supervisor cannot be started" unless @state == :registering

        @decider = Decider.new(children: @slots.size, strategy: @strategy,
                               restarts: @restarts, window_ns: to_ns(@window))
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
      @mutex.synchronize { @decider.recent_restarts(monotonic_ns) }
    end

    # True once the tree has escalated, and true from then on.
    def terminated?
      @mutex.synchronize { @state == :failed }
    end

    # Takes every running child down in reverse start order, giving the whole
    # shutdown +grace+ seconds, or all the time it needs when +grace+ is nil.
    # A tree still registering has nothing to take down and stays registering.
    #
    # Raises {Escalated} for a tree that gave up, every time it is called.
    def stop(grace: nil)
      entered = @mutex.synchronize do
        state = @state
        @state = :stopped if state == :running
        @halt.broadcast
        state
      end
      return self if entered == :registering

      @events.push(STOP)
      failures = nil
      DeadlineMachines.with_deadline(grace) do |deadline|
        failures = shutdown
        @monitor.kill unless @monitor.join(deadline.remaining)
      end
      raise Escalated, escalation if terminated?
      raise failures.first unless failures.empty?

      self
    end

    # Seconds a supervisor taking itself down waits for a killed child, since
    # neither the monitor thread nor a failing {#start} has a caller budget to
    # read.
    ESCALATION_GRACE = 5.0

    private

    # Brings every child up in registration order, returning what a starter
    # raised, or nil once the whole tree is up.
    def launch
      @mutex.synchronize do
        @slots.each_index { |index| spawn(index) }
        nil
      rescue Exception => e
        @state = :failed
        @failed = unstarted(e)
        e
      end
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
      decision = @mutex.synchronize do
        child = @slots[index]
        next unless running? && child.epoch == epoch

        child.thread = nil
        child.done = true if child.policy == :temporary
        @decider.stopped(index: index, policy: child.policy, exit: ending, now_ns: monotonic_ns)
      end

      case decision
      when Range then restart(decision)
      when :escalate then escalate
      end
    end

    # Retires the whole span in reverse start order and waits for it to go,
    # waits out the backoff without holding the tree, then brings the span back
    # in index order.
    def restart(span)
      retiring = @mutex.synchronize { span.reverse_each.filter_map { |index| retire(@slots[index]) } }
      DeadlineMachines.with_deadline(ESCALATION_GRACE) do |deadline|
        retiring.each { |thread| reap(thread, deadline.remaining) }
      end
      return escalate(abandoned) if retiring.any?(&:alive?)

      failure = @mutex.synchronize { respawn(span) }
      escalate(failure) if failure
    end

    # The reason a respawn could not happen, or nil once the span is running.
    def respawn(span)
      delay = @backoff.next_delay(attempt: @decider.recent_restarts(monotonic_ns))
      @halt.wait(@mutex, delay) if delay
      return unless running?

      span.each { |index| spawn(index) unless @slots[index].done }
      nil
    rescue Exception => e
      unstarted(e)
    end

    def escalate(reason = escalation)
      @state = :failed
      @failed = reason
      shutdown(ESCALATION_GRACE)
      raise Escalated, reason
    end

    # Reverse start order, each child killed and then waited for inside one
    # budget shared by the whole tree. Touches no tree state, so a shutdown
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

    # Kills the thread and waits out the rest of the budget for it, reporting
    # whatever it raised on the way out. A thread that outlives the budget is
    # still alive afterwards.
    def reap(thread, remaining)
      thread.kill
      thread.join(remaining)
      nil
    rescue Exception => e
      e
    end

    def spawn(index)
      child = @slots[index]
      child.thread = child.starter.call
      watch(index, child.epoch, child.thread)
    end

    # Invalidates the watcher following the child and hands its thread back to
    # be reaped outside the lock.
    def retire(child)
      child.epoch += 1
      thread = child.thread
      child.thread = nil
      thread
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
      @failed || "#{@strategy} supervisor escalated: more than #{@restarts} restarts in #{@window} seconds"
    end

    def abandoned
      "#{@strategy} supervisor escalated: a retired child would not stop within #{ESCALATION_GRACE} seconds"
    end

    def unstarted(error)
      "#{@strategy} supervisor escalated: #{error.message}"
    end

    def monotonic_ns
      DeadlineMachines.clock.now_ns
    end

    def to_ns(seconds)
      DeadlineMachines::Deadline.to_ns(seconds)
    end
  end
end
