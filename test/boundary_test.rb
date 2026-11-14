# frozen_string_literal: true

require 'test_helper'

module SupervisorMachines
  # The native boundary is the authority: sealing stops accidents, and a value
  # forged past it is refused before it reaches the core.
  class BoundaryTest < Test
    VALUES = [Intensity, Span, Restart, Supervisor.const_get(:Child)].freeze

    # Ruby binds a named subclass's constant before `inherited` raises, so the
    # constant leaks; it is still refused wherever it is read.
    module Forged; end

    def forge(value, name)
      return Forged.const_get(name, false) if Forged.const_defined?(name, false)

      assert_raises(TypeError) { Forged.const_set(name, Class.new(value)) }
      Forged.const_set("#{name}Base", value) # Child is a private constant
      assert_raises(TypeError) { Forged.module_eval("class #{name} < #{name}Base; end", __FILE__, __LINE__) }
      forged = Forged.const_get(name, false)
      forged.define_method(:initialize) { |**kw| Data.instance_method(:initialize).bind_call(self, **kw) }
      forged
    end

    def test_every_value_is_sealed_against_anonymous_and_named_subclasses
      VALUES.each_with_index do |value, index|
        forged = forge(value, "Sealed#{index}")

        assert_operator forged, :<, value, 'the constant leaks: Ruby binds it before inherited raises'
        assert_raises(NoMethodError) { value.allocate }
      end
      assert_raises(TypeError) { Decider.allocate }
    end

    def test_a_forged_subclass_value_is_a_type_error
      intensity = forge(Intensity, 'Intensity').new(restarts: 1, window: 0)
      span = forge(Span, 'Span').new(first: 5, last: 1)

      assert_raises(TypeError) { Decider.new(1, :one_for_one, intensity) }
      assert_raises(TypeError) { Decider.restore(1, :one_for_one, intensity, [], 0) }
      assert_raises(TypeError) { Supervisor.new(:one_for_one, intensity) }
      assert_raises(TypeError) { Restart.new(span) }
    end

    def test_a_marshal_edited_value_is_revalidated_into_a_range_error
      intensity = Marshal.load(Marshal.dump(Intensity.new(2, 60)).sub('iA'.b, "i\x00".b))

      assert_instance_of Intensity, intensity
      assert_equal 0, intensity.window
      assert_raises(RangeError) { Decider.new(1, :one_for_one, intensity) }
      assert_raises(RangeError) { Decider.restore(1, :one_for_one, intensity, [], 0) }
      assert_raises(RangeError) { Supervisor.new(:one_for_one, intensity) }
      span = Marshal.load(Marshal.dump(Span.new(1, 2)).sub("i\x07".b, "i\x00".b))

      assert_equal [1, 0], [span.first, span.last]
      assert_raises(RangeError) { Restart.new(span) }
    end
    def test_classes_held_by_the_binding_survive_gc_compaction
      decider = Decider.new(1, :one_for_one, Intensity.new(3, 60_000_000_000))
      # Compacting moves every unpinned object: a class the binding holds
      # unpinned would be a dangling reference and the next build would crash.
      GC.verify_compaction_references(expand_heap: true, toward: :empty)

      assert_equal Intensity.new(3, 60_000_000_000), decider.intensity
    end

  end
end
