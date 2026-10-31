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

    def test_stop_takes_every_child_down_even_when_a_teardown_raises
      supervisor = Supervisor.new(backoff: IMMEDIATE)
      supervisor.supervise(:first, &@nursery.starter(:first))
      supervisor.supervise(:last, &@nursery.raising_starter(:last))
      supervisor.start

      assert_equal %i[first last], @nursery.starts(2)

      assert_raises(Nursery::Failure) { supervisor.stop(grace: Nursery::PATIENCE) }
      assert_equal %i[first], @nursery.stops(1)
    end

    def test_stop_spends_one_grace_on_the_whole_tree
      supervisor = Supervisor.new(backoff: IMMEDIATE)
      3.times { |index| supervisor.supervise(index, &@nursery.clinging_starter(index)) }
      supervisor.start
      @nursery.starts(3)

      started = monotonic
      supervisor.stop(grace: Nursery::QUIET)

      assert_operator monotonic - started, :<, Nursery::QUIET * 3
    end

    def test_a_starter_that_cannot_run_leaves_the_tree_terminated
      supervisor = Supervisor.new(backoff: IMMEDIATE)
      attempts = 0
      supervisor.supervise(:doomed) do
        attempts += 1
        raise Nursery::Failure if attempts > 1

        @nursery.starter(:doomed).call
      end
      supervisor.start

      assert_equal :doomed, @nursery.next_start

      @nursery.finish(:doomed, :abnormal)

      assert_equal :doomed, @nursery.next_stop
      wait_until { supervisor.terminated? }
      assert_raises(Escalated) { supervisor.stop(grace: Nursery::PATIENCE) }
    end

    def test_a_group_restart_waits_for_the_generation_it_retires
      supervisor = tree(%i[a b], strategy: :one_for_all)

      assert_equal %i[a b], @nursery.starts(2)

      @nursery.finish(:a, :abnormal)

      assert_equal %i[a b], @nursery.stops(2)
      assert_equal %i[a b], @nursery.starts(2)

      supervisor.stop(grace: Nursery::PATIENCE)
    end

    private

    def wait_until
      deadline = monotonic + Nursery::PATIENCE
      sleep 0.01 until yield || monotonic > deadline
    end

    def monotonic
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end
  end
end
