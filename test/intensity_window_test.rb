# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class IntensityWindowTest < Test
    def setup
      super
      @clock = DeadlineMachines::TestClock.new(now_ns: 1_000_000_000)
      DeadlineMachines.clock = @clock
    end

    def teardown
      DeadlineMachines.clock = nil
      super
    end

    def test_the_restart_window_is_driven_by_the_shared_clock
      supervisor = tree(%i[a], restarts: 1)

      assert_equal %i[a], @nursery.starts(1)

      @nursery.finish(:a, :abnormal)
      assert_equal :a, @nursery.next_start
      assert_equal 1, supervisor.restarts

      @clock.advance(120.0)

      assert_equal 0, supervisor.restarts
      refute_predicate supervisor, :terminated?

      supervisor.stop(grace: Nursery::PATIENCE)
    end
  end
end
