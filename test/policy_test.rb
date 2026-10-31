# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class PolicyTest < Test
    def test_a_permanent_child_comes_back_from_a_clean_exit
      supervisor = tree(%i[a b])

      assert_equal %i[a b], @nursery.starts(2)

      @nursery.finish(:b)

      assert_equal :b, @nursery.next_start
      supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_a_transient_child_is_left_alone_after_a_clean_exit
      supervisor = tree(%i[a b], policy: :transient)

      assert_equal %i[a b], @nursery.starts(2)

      @nursery.finish(:b)

      assert_equal :b, @nursery.next_stop
      assert_nil @nursery.next_start(Nursery::QUIET)
      supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_a_transient_child_comes_back_from_a_bad_exit
      supervisor = tree(%i[a b], policy: :transient)

      assert_equal %i[a b], @nursery.starts(2)

      @nursery.finish(:b, :abnormal)

      assert_equal :b, @nursery.next_start
      supervisor.stop(grace: Nursery::PATIENCE)
    end

    def test_a_temporary_child_is_never_restarted
      supervisor = tree(%i[a b], policy: :temporary)

      assert_equal %i[a b], @nursery.starts(2)

      @nursery.finish(:b, :abnormal)

      assert_equal :b, @nursery.next_stop

      @nursery.finish(:a)

      assert_equal :a, @nursery.next_stop
      assert_nil @nursery.next_start(Nursery::QUIET)
      refute_predicate supervisor, :terminated?
      supervisor.stop(grace: Nursery::PATIENCE)
    end
  end
end
