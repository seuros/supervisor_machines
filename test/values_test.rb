# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  class ValuesTest < Test
    def test_an_intensity_holds_what_the_core_would_build
      intensity = Intensity.new(3, MINUTE)

      assert_equal [3, MINUTE], [intensity.restarts, intensity.window]
      assert_equal Intensity.new(restarts: 3, window: MINUTE), intensity
      assert_equal Intensity.new(DEFAULT_RESTARTS, 5 * SECOND), Intensity.default
    end

    def test_a_span_covers_at_least_one_child
      span = Span.new(2, 5)

      assert_equal [2, 5, 4], [span.first, span.last, span.size]
      refute_empty span
      assert span.contains?(2) && span.contains?(5)
      refute span.contains?(6)
      assert_equal Span.new(4, 4), Span.single(4)
      assert_equal span, Restart.new(span).span
    end

    # Wrong type is a TypeError, a value the core's constructor rejects a
    # RangeError, for values built by hand as for those the gem returns.
    def test_every_value_is_validated_through_the_core
      {
        RangeError => [
          -> { Intensity.new(3, 0) }, -> { Intensity.new(3, -1) }, -> { Intensity.new(-1, MINUTE) },
          -> { Intensity.new(2**32, MINUTE) }, -> { Intensity.new(3, 2**64) },
          -> { Span.new(5, 2) }, -> { Span.new(-1, 2) }, -> { Span.new(1, 2).with(last: 0) }
        ],
        TypeError => [
          -> { Intensity.new(3.9, MINUTE) }, -> { Intensity.new(3, 60.0) }, -> { Intensity.new(nil, MINUTE) },
          -> { Span.new(1.0, 2) }, -> { Restart.new(0..2) }, -> { Restart.new(3) }
        ]
      }.each do |error, builds|
        builds.each { |build| assert_raises(error) { build.call } }
      end
    end

    # The defect: a subclass overriding `initialize` without `super`, or
    # `allocate`, built a value that never ran the check.
    def test_values_are_final
      [Intensity, Span, Restart, Supervisor.const_get(:Child)].each do |value|
        assert_raises(TypeError) { Class.new(value) { def initialize(**) = nil } }
        assert_raises(NoMethodError) { value.allocate }
      end
    end

    # The native wrappers have no allocator, and a subclass's `new` would
    # build the base class anyway: subclassing them raises too.
    def test_native_wrappers_are_sealed
      [Decider].each do |wrapper|
        assert_raises(TypeError) { Class.new(wrapper) }
        assert_raises(TypeError) { wrapper.allocate }
      end
    end

    def test_the_closed_sets_are_the_cores
      assert_equal %i[one_for_one one_for_all rest_for_one], STRATEGIES
      assert_equal %i[permanent transient temporary], POLICIES
      assert_equal %i[normal abnormal], EXITS
      assert_predicate STRATEGIES, :frozen?
    end
  end
end
