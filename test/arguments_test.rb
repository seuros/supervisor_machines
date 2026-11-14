# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class ArgumentsTest < Test
    def fresh(strategy = :one_for_one)
      Supervisor.new(strategy, Intensity.default, backoff: IMMEDIATE)
    end

    def test_a_tree_is_built_from_what_the_core_accepts
      assert_raises(RangeError) { Supervisor.new(:one_for_some, Intensity.default) }
      assert_raises(TypeError) { Supervisor.new('one_for_one', Intensity.default) }
      assert_raises(TypeError) { Supervisor.new(:one_for_one, [1, MINUTE]) }
      error = assert_raises(RangeError) do
        Supervisor.new(:one_for_one, Intensity.new(Decider::MAX_RESTARTS + 1, MINUTE))
      end
      assert_includes error.message, Decider::MAX_RESTARTS.to_s

      supervisor = Supervisor.new(:rest_for_one, Intensity.new(3, MINUTE))

      assert_equal [:rest_for_one, Intensity.new(3, MINUTE), 0],
                   [supervisor.strategy, supervisor.intensity, supervisor.restarts]
    end

    def test_an_unknown_policy_is_out_of_range
      supervisor = fresh

      assert_raises(RangeError) { supervisor.supervise(:a, policy: :eternal, &@nursery.starter(:a)) }
      assert_raises(TypeError) { supervisor.supervise(:a, policy: 'permanent', &@nursery.starter(:a)) }
      assert_empty supervisor.children
    end

    def test_a_child_count_without_a_shape_is_out_of_range
      supervisor = fresh
      (Decider::SUPPORTED_CHILDREN.max - 1).times { |index| supervisor.supervise(index, &@nursery.starter(index)) }

      assert_raises(RangeError) { supervisor.start }
    end

    def test_a_tree_with_no_children_is_out_of_range
      assert_raises(RangeError) { fresh.start }
    end

    def test_a_started_tree_takes_on_no_more_children
      supervisor = tree(%i[a])

      assert_equal :a, @nursery.next_start
      assert_raises(Error) { supervisor.supervise(:late, &@nursery.starter(:late)) }
      assert_equal %i[a], supervisor.children
      assert_nil @nursery.next_start(Nursery::QUIET)

      supervisor.stop(Nursery::PATIENCE_NS)
    end

    def test_a_grace_is_integer_nanoseconds_checked_before_anything_stops
      supervisor = tree(%i[a])

      assert_equal :a, @nursery.next_start
      assert_raises(TypeError) { supervisor.stop(2.0) }
      assert_raises(RangeError) { supervisor.stop(-1) }
      assert_nil @nursery.next_stop(Nursery::QUIET)

      supervisor.stop(Nursery::PATIENCE_NS)

      assert_equal :a, @nursery.next_stop
    end
  end
end
