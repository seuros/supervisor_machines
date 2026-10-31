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

    def test_a_start_that_cannot_finish_takes_back_the_children_it_brought_up
      supervisor = half_started_tree

      assert_raises(Nursery::Failure) { supervisor.start }
      assert_equal :up, @nursery.next_start
      assert_equal :up, @nursery.next_stop
      assert_predicate supervisor, :terminated?
    end

    def test_stop_after_a_start_that_cannot_finish_stays_escalated
      supervisor = half_started_tree

      assert_raises(Nursery::Failure) { supervisor.start }
      assert_raises(Escalated) { supervisor.stop(grace: Nursery::PATIENCE) }
      assert_raises(Escalated) { supervisor.stop(grace: Nursery::PATIENCE) }
    end

    def test_stop_without_a_start_has_nothing_to_take_down
      supervisor = Supervisor.new(backoff: IMMEDIATE)
      supervisor.supervise(:a, &@nursery.starter(:a))

      assert_same supervisor, supervisor.stop(grace: Nursery::PATIENCE)
      assert_nil @nursery.next_start(Nursery::QUIET)
      refute_predicate supervisor, :terminated?
    end

    def test_a_second_start_is_refused_and_leaves_one_generation_running
      supervisor = tree(%i[a])

      assert_equal %i[a], @nursery.starts(1)
      assert_raises(Error) { supervisor.start }
      assert_nil @nursery.next_start(Nursery::QUIET)

      supervisor.stop(grace: Nursery::PATIENCE)

      assert_equal %i[a], @nursery.stops(1)
      assert_nil @nursery.next_stop(Nursery::QUIET)
      assert_raises(Error) { supervisor.start }
    end

    def test_a_group_restart_waits_for_the_generation_it_retires
      supervisor = tree(%i[a b], strategy: :one_for_all)

      assert_equal %i[a b], @nursery.starts(2)

      @nursery.finish(:a, :abnormal)

      assert_equal %i[a b], @nursery.stops(2)
      assert_equal %i[a b], @nursery.starts(2)

      supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_a_starter_can_ask_the_tree_that_is_starting_it_about_itself
      supervisor = Supervisor.new(backoff: IMMEDIATE)
      seen = nil
      supervisor.supervise(:curious) do
        seen = [supervisor.children, supervisor.restarts, supervisor.terminated?]
        @nursery.starter(:curious).call
      end
      supervisor.start

      assert_equal [%i[curious], 0, false], seen
      assert_equal :curious, @nursery.next_start
      supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_a_stop_while_a_starter_is_blocked_spends_only_its_grace
      gate = Thread::Queue.new
      supervisor = blocked_start(gate)
      release_after(Nursery::PATIENCE, gate)

      started = monotonic
      supervisor.stop(grace: Nursery::QUIET)

      assert_operator monotonic - started, :<, Nursery::PATIENCE
    end

    def test_a_starter_returning_after_a_stop_has_its_thread_taken_down
      gate = Thread::Queue.new
      supervisor = blocked_start(gate)
      release_after(Nursery::QUIET, gate)
      supervisor.stop(grace: Nursery::PATIENCE)

      assert_equal :blocking, @nursery.next_start
      assert_equal :blocking, @nursery.next_stop
      refute_predicate supervisor, :terminated?
    end

    private

    # Lets a blocked starter return +seconds+ from now.
    def release_after(seconds, gate)
      Thread.new do
        sleep seconds
        gate.push(:go)
      end
    end

    # A tree whose one starter has been called and will not return until +gate+
    # is pushed.
    def blocked_start(gate)
      Supervisor.new(backoff: IMMEDIATE).tap do |supervisor|
        entered = Thread::Queue.new
        supervisor.supervise(:blocking) do
          entered.push(:in)
          gate.pop
          @nursery.starter(:blocking).call
        end
        Thread.new { supervisor.start }
        entered.pop
      end
    end

    # An unstarted tree whose second child's starter raises, so the first child
    # is up by the time the start fails.
    def half_started_tree
      Supervisor.new(backoff: IMMEDIATE).tap do |supervisor|
        supervisor.supervise(:up, &@nursery.starter(:up))
        supervisor.supervise(:doomed) { raise Nursery::Failure }
      end
    end

    def wait_until
      deadline = monotonic + Nursery::PATIENCE
      sleep 0.01 until yield || monotonic > deadline
    end

    def monotonic
      Process.clock_gettime(Process::CLOCK_MONOTONIC)
    end
  end
end
