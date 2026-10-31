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
  #   tree = SupervisorMachines::Supervisor.new(strategy: :one_for_all, restarts: 3, window: 60.0)
  #   tree.supervise(:cache) { Thread.new { cache.run } }
  #   tree.supervise(:warmup, policy: :transient) { Thread.new { warm } }
  #   tree.start
  #   tree.stop(grace: 5.0)
  class Supervisor
    # A registered child and whichever thread currently runs it. +epoch+ rises
    # every time the child is taken down, so an exit reported by a thread the
    # tree has already replaced is discarded.
    Child = Struct.new(:name, :policy, :starter, :thread, :epoch)

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

      @strategy = strategy
      @restarts = restarts
      @window = window
      @backoff = backoff || ChronoMachines::Executor.new(max_attempts: restarts + 1)
      @slots = []
      @events = Thread::Queue.new
      @mutex = Thread::Mutex.new
      @halt = Thread::ConditionVariable.new
      @stopping = false
    end

    # Registers a child under +name+. The block is its starter: it returns the
    # +Thread+ that runs the child, and is called again for every restart.
    def supervise(name, policy: :permanent, &starter)
      raise ArgumentError, "unknown policy: #{policy.inspect}" unless POLICIES.include?(policy)

      @mutex.synchronize { @slots << Child.new(name, policy, starter, nil, 0) }
      self
    end

    # Fixes the shape of the tree, starts every child in registration order and
    # begins watching them.
    def start
      @mutex.synchronize do
        @decider = Decider.new(children: @slots.size, strategy: @strategy,
                               restarts: @restarts, window_ns: to_ns(@window))
        @slots.each_index { |index| spawn(index) }
      end
      @monitor = Thread.new { monitor }
      self
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
      @mutex.synchronize { @decider.terminated? }
    end

    # Takes every running child down in reverse start order, giving the whole
    # shutdown +grace+ seconds, or all the time it needs when +grace+ is nil.
    #
    # Raises {Escalated} for a tree that gave up, every time it is called.
    def stop(grace: nil)
      @mutex.synchronize do
        @stopping = true
        @halt.broadcast
      end
      @events.push(STOP)
      DeadlineMachines.with_deadline(grace) do |deadline|
        shutdown
        @monitor.kill unless @monitor.join(deadline.remaining)
      end
      raise Escalated, escalation if terminated?

      self
    end

    # Seconds an escalating supervisor waits for a killed child, since the
    # monitor thread has no caller budget to read.
    ESCALATION_GRACE = 5.0

    private

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
        next if @stopping || child.epoch != epoch

        child.thread = nil
        @decider.stopped(index: index, policy: child.policy, exit: ending, now_ns: monotonic_ns)
      end

      case decision
      when Range then restart(decision)
      when :escalate then escalate
      end
    end

    # Retires the whole span, waits out the backoff without holding the tree,
    # then brings the span back in index order.
    def restart(span)
      @mutex.synchronize do
        span.each { |index| retire(@slots[index]) }
        delay = @backoff.next_delay(attempt: @decider.recent_restarts(monotonic_ns))
        @halt.wait(@mutex, delay) if delay
        next if @stopping

        span.each { |index| spawn(index) }
      end
    end

    def escalate
      @stopping = true
      shutdown(ESCALATION_GRACE)
      raise Escalated, escalation
    end

    # Reverse start order, each child killed and then waited for inside
    # whatever budget the caller set. Touches no tree state, so a shutdown
    # under way in the monitor never holds up the one in {#stop}.
    def shutdown(grace = DeadlineMachines.remaining)
      @slots.reverse_each do |child|
        thread = child.thread
        next unless thread

        thread.kill
        thread.join(grace)
      end
    end

    def spawn(index)
      child = @slots[index]
      child.thread = child.starter.call
      watch(index, child.epoch, child.thread)
    end

    # Takes the child's thread down and invalidates the watcher following it.
    def retire(child)
      child.epoch += 1
      thread = child.thread
      child.thread = nil
      thread&.kill
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
      "#{@strategy} supervisor escalated: more than #{@restarts} restarts in #{@window} seconds"
    end

    def monotonic_ns
      DeadlineMachines.clock.now_ns
    end

    def to_ns(seconds)
      DeadlineMachines::Deadline.to_ns(seconds)
    end
  end
end
