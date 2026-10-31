# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class LifecycleTest < Test
    def test_stop_takes_children_down_in_reverse_start_order
      supervisor = tree(%i[a b c d])

      assert_equal %i[a b c d], @nursery.starts(4)

      supervisor.stop(grace: Nursery::PATIENCE)

      assert_equal %i[d c b a], @nursery.stops(4)
    end

    def test_stop_gives_up_on_a_child_that_outlasts_the_grace
      supervisor = Supervisor.new(backoff: IMMEDIATE)
      supervisor.supervise(:clinging, &@nursery.clinging_starter(:clinging))
      supervisor.supervise(:quick, &@nursery.starter(:quick))
      supervisor.start

      assert_equal %i[clinging quick], @nursery.starts(2)

      started = monotonic
      supervisor.stop(grace: Nursery::QUIET)

      assert_operator monotonic - started, :<, Nursery::PATIENCE
    end

    def test_escalating_takes_the_tree_down_and_stays_escalated
      supervisor = tree(%i[a b c], restarts: 1)

      assert_equal %i[a b c], @nursery.starts(3)

      @nursery.finish(:b, :abnormal)

      assert_equal :b, @nursery.next_stop
      assert_equal :b, @nursery.next_start

      @nursery.finish(:b, :abnormal)

      assert_equal %i[b c a], @nursery.stops(3)
      assert_predicate supervisor, :terminated?
      assert_raises(Escalated) { supervisor.stop(grace: Nursery::PATIENCE) }
      assert_raises(Escalated) { supervisor.stop(grace: Nursery::PATIENCE) }
      assert_nil @nursery.next_start(Nursery::QUIET)
    end

    private

    def monotonic
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end
  end
end
