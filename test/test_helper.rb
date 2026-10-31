# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

require 'supervisor_machines'
require 'minitest/autorun'

module SupervisorMachines
  # Children that announce every start, park until told how to end, and
  # announce every teardown.
  class Nursery
    # How long a test waits for something it expects to happen.
    PATIENCE = 2.0

    # How long a test waits to be convinced nothing is going to happen.
    QUIET = 0.2

    # What an abnormal child raises on its way out.
    class Failure < StandardError; end

    def initialize
      @starts = Thread::Queue.new
      @stops = Thread::Queue.new
      @gates = {}
    end

    # The starter block for +name+: parks until #finish ends it, cleanly or
    # not.
    def starter(name)
      gate = (@gates[name] = Thread::Queue.new)
      starting(name) { raise Failure if gate.pop == :abnormal }
    end

    # A child whose teardown outlasts any grace period a test would set.
    def clinging_starter(name)
      starting(name, teardown: PATIENCE) { sleep }
    end

    # A starter that announces the start and does not return until its thread
    # is running inside its own teardown, so a child is never killed before it
    # could notice.
    def starting(name, teardown: 0, &body)
      lambda do
        @starts.push(name)
        running = Thread::Queue.new
        Thread.new do
          running.push(name)
          body.call
        ensure
          sleep teardown
          @stops.push(name)
        end.tap { running.pop }
      end
    end

    # Ends +name+'s current thread, cleanly or not.
    def finish(name, ending = :normal)
      @gates[name].push(ending)
    end

    # The next child to start, or nil if none does in time.
    def next_start(timeout = PATIENCE)
      @starts.pop(timeout: timeout)
    end

    # The next child to tear down, or nil if none does in time.
    def next_stop(timeout = PATIENCE)
      @stops.pop(timeout: timeout)
    end

    # The next +count+ children to start, in order.
    def starts(count)
      Array.new(count) { next_start }
    end

    # The next +count+ children to tear down, in order.
    def stops(count)
      Array.new(count) { next_stop }
    end
  end

  class Test < Minitest::Test
    # A tree restarts as fast as it can decide to, so the tests measure the
    # supervisor rather than chrono_machines' backoff curve.
    IMMEDIATE = ChronoMachines::Executor.new(base_delay: 0, max_attempts: Decider::HISTORY)

    def setup
      Thread.report_on_exception = false
      @nursery = Nursery.new
    end

    # A started tree over +names+, every child registered with +policy+.
    def tree(names, strategy: :one_for_one, policy: :permanent, restarts: Decider::HISTORY)
      tree = Supervisor.new(strategy: strategy, restarts: restarts, window: 60.0, backoff: IMMEDIATE)
      names.each { |name| tree.supervise(name, policy: policy, &@nursery.starter(name)) }
      tree.start
    end
  end
end
