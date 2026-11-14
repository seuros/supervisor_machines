# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class DeciderTest < Test
    def decider(children = 3, strategy = :one_for_one, restarts = 2, window = MINUTE)
      Decider.new(children, strategy, Intensity.new(restarts, window))
    end

    # Everything a refusal must leave as it was.
    def snapshot(decider, now)
      [decider.history, decider.recent_restarts(now), decider.terminated?]
    end

    def test_the_strategy_decides_the_span
      assert_equal Restart.new(Span.new(1, 1)), decider(4, :one_for_one).stopped(1, :permanent, :abnormal, 0)
      assert_equal Restart.new(Span.new(0, 3)), decider(4, :one_for_all).stopped(1, :permanent, :abnormal, 0)
      assert_equal Restart.new(Span.new(1, 3)), decider(4, :rest_for_one).stopped(1, :permanent, :abnormal, 0)
    end

    def test_a_stop_not_worth_restarting_is_ignored_and_records_nothing
      tree = decider

      assert_equal :ignore, tree.stopped(0, :temporary, :abnormal, SECOND)
      assert_equal :ignore, tree.stopped(0, :transient, :normal, SECOND)
      assert_empty tree.history
    end

    def test_escalating_once_then_answering_already_escalated
      tree = decider(3, :one_for_one, 1)

      assert_instance_of Restart, tree.stopped(0, :permanent, :abnormal, 0)
      assert_equal :escalate, tree.stopped(0, :permanent, :abnormal, SECOND)
      assert_predicate tree, :terminated?

      before = snapshot(tree, 2 * SECOND)

      assert_equal :already_escalated, tree.stopped(1, :permanent, :abnormal, 2 * SECOND)
      assert_equal before, snapshot(tree, 2 * SECOND)
    end

    # The defect: the core used to answer for a child it does not own; only
    # the binding raised. Now the core refuses, and the refusal is a symbol.
    def test_a_child_outside_the_tree_is_refused_and_changes_nothing
      tree = decider(2, :rest_for_one)
      tree.stopped(0, :permanent, :abnormal, 0)
      before = snapshot(tree, SECOND)

      assert_equal :unknown_child, tree.stopped(2, :permanent, :abnormal, SECOND)
      assert_equal :unknown_child, tree.stopped(2**40, :temporary, :normal, SECOND)
      assert_equal before, snapshot(tree, SECOND)
      assert_nil tree.span(2)
    end

    def test_malformed_arguments_raise_before_the_core_is_asked
      tree = decider

      assert_raises(TypeError) { tree.stopped(1.9, :permanent, :abnormal, 1) }
      assert_raises(TypeError) { tree.stopped(0, :permanent, :abnormal, 1.9) }
      assert_raises(RangeError) { tree.stopped(-1, :permanent, :abnormal, 1) }
      assert_raises(RangeError) { tree.stopped(0, :permanent, :abnormal, -1) }
      assert_raises(RangeError) { tree.stopped(0, :eternal, :abnormal, 1) }
      assert_raises(TypeError) { tree.stopped(0, 'permanent', :abnormal, 1) }
      assert_raises(RangeError) { tree.stopped(0, :permanent, :exploded, 1) }
      assert_empty tree.history
    end

    def test_a_decider_is_built_from_what_the_core_accepts
      assert_raises(TypeError) { Decider.new(1.0, :one_for_one, Intensity.default) }
      assert_raises(TypeError) { Decider.new(1, :one_for_one, [1, MINUTE]) }
      assert_raises(RangeError) { Decider.new(0, :one_for_one, Intensity.default) }
      assert_raises(RangeError) { Decider.new(17, :one_for_one, Intensity.default) }
      assert_raises(RangeError) { Decider.new(1, :one_for_some, Intensity.default) }
      assert_raises(RangeError) { decider(1, :one_for_one, Decider::HISTORY) }

      tree = decider(3, :one_for_all, 2, MINUTE)

      assert_equal [3, :one_for_all, Intensity.new(2, MINUTE)], [tree.children, tree.strategy, tree.intensity]
      assert_equal Span.new(0, 2), tree.span(1)
      refute_predicate tree, :terminated?
    end

    def test_an_external_restart_spends_the_budget
      tree = decider(3, :one_for_one, 1)

      assert_equal :recorded, tree.record_external_restart(SECOND)
      assert_equal [SECOND], tree.history
      assert_equal :escalate, tree.stopped(0, :permanent, :abnormal, 2 * SECOND)
    end

    def test_an_external_restart_older_than_a_full_history_is_outdated
      tree = decider(1, :one_for_one, Decider::MAX_RESTARTS, 2**62)
      Decider::HISTORY.times { |at| tree.record_external_restart(SECOND + at) }
      before = snapshot(tree, SECOND)

      assert_equal :outdated, tree.record_external_restart(0)
      assert_equal before, snapshot(tree, SECOND)
    end

    def test_absorbing_merges_and_counts_nothing_twice
      tree = decider
      tree.stopped(0, :permanent, :abnormal, 10 * SECOND)

      assert_equal 1, tree.absorb([10 * SECOND, 20 * SECOND], 21 * SECOND)
      assert_equal [10 * SECOND, 20 * SECOND], tree.history
      assert_equal 0, tree.absorb([10 * SECOND, 20 * SECOND], 22 * SECOND)
      assert_predicate tree.history, :frozen?
    end

    # Another process's two restarts in one nanosecond stay two; the merge
    # keeps the larger count of each instant, so repeats change nothing.
    def test_absorbing_keeps_the_larger_count_of_each_instant
      tree = decider
      tree.record_external_restart(10 * SECOND)

      assert_equal 1, tree.absorb([10 * SECOND, 10 * SECOND], 11 * SECOND)
      assert_equal [10 * SECOND, 10 * SECOND], tree.history
      before = snapshot(tree, 12 * SECOND)

      assert_equal 0, tree.absorb([10 * SECOND, 10 * SECOND], 12 * SECOND)
      assert_equal 0, tree.absorb([10 * SECOND], 12 * SECOND)
      assert_equal before, snapshot(tree, 12 * SECOND)
    end

    # The binding never loses work: a malformed history raises before the core
    # absorbs any of it.
    def test_a_malformed_history_absorbs_nothing
      tree = decider

      assert_raises(TypeError) { tree.absorb([SECOND, 2.5], 3 * SECOND) }
      assert_raises(RangeError) { tree.absorb([SECOND, -1], 3 * SECOND) }
      assert_raises(TypeError) { tree.absorb(SECOND, 3 * SECOND) }
      assert_empty tree.history
    end

    def test_restoring_a_history_rebuilds_the_window
      first = decider
      first.stopped(0, :permanent, :abnormal, 10 * SECOND)
      first.stopped(0, :permanent, :abnormal, 20 * SECOND)

      second = Decider.restore(3, :one_for_one, first.intensity, first.history, 25 * SECOND)

      assert_equal first.history, second.history
      assert_equal :escalate, second.stopped(0, :permanent, :abnormal, 30 * SECOND)
      assert_raises(TypeError) { Decider.restore(3, :one_for_one, first.intensity, [1.5], 0) }
    end

    # Saved state is a multiset: two restarts at one instant restore as two.
    def test_restoring_keeps_two_restarts_at_one_instant
      first = decider(1, :one_for_one, 2)
      first.record_external_restart(10 * SECOND)
      first.record_external_restart(10 * SECOND)

      second = Decider.restore(1, :one_for_one, first.intensity, first.history, 20 * SECOND)

      assert_equal [10 * SECOND, 10 * SECOND], second.history
      assert_equal :escalate, second.stopped(0, :permanent, :abnormal, 20 * SECOND)
    end

    def test_the_restart_delay_backs_off_in_nanoseconds
      tree = decider(1, :one_for_one, 5)
      tree.stopped(0, :permanent, :abnormal, 0)
      tree.stopped(0, :permanent, :abnormal, 1)

      assert_equal 200, tree.restart_delay(100, 10_000, 1)
      assert_equal 100, tree.restart_delay(100, 10_000, 2 * MINUTE)
      assert_raises(TypeError) { tree.restart_delay(0.1, 10_000, 1) }
    end

    # Runs the hook set for the current thread before every `Restart.new`.
    # The class is sealed, not frozen, so a caller can still do this.
    HOOKED = Module.new do
      def new(*)
        Thread.current[:restart_hook]&.call
        super
      end
    end
    Restart.singleton_class.prepend(HOOKED)

    def redefining_restart(hook)
      Thread.current[:restart_hook] = hook
      yield
    ensure
      Thread.current[:restart_hook] = nil
    end

    # The restart is decided on a copy: a raise while its answer is built
    # records nothing, so the next call decides afresh.
    def test_a_raise_while_building_the_answer_records_nothing
      tree = decider

      redefining_restart(-> { raise IOError, 'no answer this time' }) do
        assert_raises(IOError) { tree.stopped(0, :permanent, :abnormal, SECOND) }
      end
      assert_empty tree.history
      assert_equal Restart.new(Span.new(0, 0)), tree.stopped(0, :permanent, :abnormal, SECOND)
    end

    def test_a_reentrant_change_while_building_raises_and_the_outer_call_is_not_applied
      tree = decider

      redefining_restart(-> { tree.record_external_restart(5 * SECOND) }) do
        assert_raises(RuntimeError) { tree.stopped(0, :permanent, :abnormal, SECOND) }
      end
      assert_equal [5 * SECOND], tree.history
    end

    def test_freezing_the_decider_while_building_raises_and_nothing_is_applied
      tree = decider

      redefining_restart(-> { tree.freeze }) do
        assert_raises(FrozenError) { tree.stopped(0, :permanent, :abnormal, SECOND) }
      end
      assert_empty tree.history
      refute_predicate tree, :terminated?
    end

    # A reader copies the core value out before it builds its answer, so an
    # `Intensity.new` that calls back into the decider is an ordinary call.
    def test_a_reader_whose_answer_re_enters_the_decider_is_not_fatal
      tree = decider
      original = Intensity.method(:new)
      Intensity.define_singleton_method(:new) do |*args, **kw|
        tree.record_external_restart(SECOND)
        original.call(*args, **kw)
      end

      assert_equal Intensity.new(2, MINUTE), tree.intensity
      assert_equal [SECOND, SECOND], tree.history
    ensure
      Intensity.define_singleton_method(:new, original)
    end

    def test_a_frozen_decider_refuses_to_change
      tree = decider.freeze

      assert_raises(FrozenError) { tree.stopped(0, :permanent, :abnormal, 0) }
      assert_raises(FrozenError) { tree.record_external_restart(0) }
      assert_raises(FrozenError) { tree.absorb([0], 0) }
      assert_empty tree.history
    end
  end
end
