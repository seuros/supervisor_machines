# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class WholeNumberTest < Test
    def test_a_fractional_intensity_or_reading_is_refused_rather_than_truncated
      assert_raises(TypeError) do
        Decider.new(children: 1, strategy: :one_for_one, restarts: 3.9, window_ns: 1000)
      end

      decider = Decider.new(children: 2, strategy: :one_for_one, restarts: 3, window_ns: 1000)

      assert_raises(TypeError) do
        decider.stopped(index: 1.9, policy: :permanent, exit: :abnormal, now_ns: 1)
      end
      assert_raises(TypeError) do
        decider.stopped(index: 0, policy: :permanent, exit: :abnormal, now_ns: 1.9)
      end

      assert_equal 0, decider.recent_restarts(1)
    end
  end
end
