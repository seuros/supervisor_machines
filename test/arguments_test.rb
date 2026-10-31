# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class ArgumentsTest < Test
    def test_an_unknown_strategy_is_rejected
      assert_raises(ArgumentError) { Supervisor.new(strategy: :one_for_some) }
    end

    def test_an_unknown_policy_is_rejected
      supervisor = Supervisor.new

      assert_raises(ArgumentError) { supervisor.supervise(:a, policy: :eternal, &@nursery.starter(:a)) }
    end

    def test_a_child_count_without_a_shape_is_rejected
      supervisor = Supervisor.new
      (Decider::SUPPORTED_CHILDREN.max - 1).times { |index| supervisor.supervise(index, &@nursery.starter(index)) }

      assert_raises(ArgumentError) { supervisor.start }
    end

    def test_a_tree_with_no_children_is_rejected
      assert_raises(ArgumentError) { Supervisor.new.start }
    end

    def test_a_child_index_outside_the_tree_is_refused
      decider = Decider.new(children: 2, strategy: :rest_for_one, restarts: 3, window_ns: 1_000)

      assert_raises(ArgumentError) do
        decider.stopped(index: 2, policy: :permanent, exit: :abnormal, now_ns: 0)
      end
      assert_equal 0, decider.recent_restarts(0)
    end

    def test_an_intensity_the_history_cannot_enforce_is_refused
      error = assert_raises(ArgumentError) { Supervisor.new(restarts: Decider::MAX_RESTARTS + 1) }

      assert_includes error.message, Decider::MAX_RESTARTS.to_s
    end
  end
end
