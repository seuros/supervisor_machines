# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class StrategyTest < Test
    def test_one_for_one_restarts_only_the_child_that_died
      supervisor = tree(%i[a b c])

      assert_equal %i[a b c], @nursery.starts(3)

      @nursery.finish(:b, :abnormal)

      assert_equal :b, @nursery.next_start
      assert_nil @nursery.next_start(Nursery::QUIET)
      assert_same supervisor, supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_one_for_all_restarts_every_child_whichever_one_died
      supervisor = tree(%i[a b c], strategy: :one_for_all)

      assert_equal %i[a b c], @nursery.starts(3)

      @nursery.finish(:b, :abnormal)

      assert_equal %i[a b c], @nursery.starts(3)
      assert_nil @nursery.next_start(Nursery::QUIET)
      supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_rest_for_one_restarts_the_child_and_everything_started_after_it
      supervisor = tree(%i[a b c d], strategy: :rest_for_one)

      assert_equal %i[a b c d], @nursery.starts(4)

      @nursery.finish(:b, :abnormal)

      assert_equal %i[b c d], @nursery.starts(3)
      assert_nil @nursery.next_start(Nursery::QUIET)
      supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_the_largest_shape_restarts_the_whole_set
      names = (1..32).to_a
      supervisor = tree(names, strategy: :one_for_all)

      assert_equal names, @nursery.starts(names.size)

      @nursery.finish(17, :abnormal)

      assert_equal names, @nursery.starts(names.size)
      supervisor.stop(grace: Nursery::PATIENCE)
    end
  end
end
