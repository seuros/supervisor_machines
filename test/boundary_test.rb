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

    # A singleton class opened on a genuine value does not change what it is.
    def test_an_opened_singleton_class_is_still_the_gems_value
      intensity = Intensity.new(2, 60).tap(&:singleton_class)

      assert_equal Intensity.new(2, 60), Decider.new(1, :one_for_one, intensity).intensity
      assert_equal Intensity.new(2, 60), Decider.restore(1, :one_for_one, intensity, [5], 10).intensity
      assert_equal Intensity.new(2, 60), Supervisor.new(:one_for_one, intensity).intensity
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

    # Fields are read from the struct itself: a redefined reader is never
    # called, so it can neither lie nor re-enter. `Restart` checks its span
    # natively, not with a Ruby `instance_of?`.
    def test_a_redefined_reader_is_ignored
      intensity = Intensity.new(2, 60)
      span = Span.new(1, 2)
      redefined(Intensity => %i[restarts window], Span => %i[first last]) do
        assert_equal 1, Decider.new(1, :one_for_one, intensity).children
        assert_equal 1, Decider.restore(1, :one_for_one, intensity, [5], 10).children
        Restart.new(span)
      end
      forger = forge(Span, 'Span')
      forger.define_method(:instance_of?) { |_| true }
      forger.define_method(:class) { Span }

      assert_raises(TypeError) { Restart.new(forger.new(first: 1, last: 2)) }
    end

    # `Supervisor.new` reads its intensity natively, as `Decider.new` does:
    # neither `instance_of?` nor a reader is asked, so neither can lie.
    def test_the_supervisor_reads_its_intensity_natively
      intensity = Intensity.new(2, 60)
      Intensity.alias_method(:__real_instance_of?, :instance_of?)
      Intensity.define_method(:instance_of?) { |_| false }
      redefined(Intensity => %i[restarts window]) do
        assert_equal [2, 60], Supervisor.new(:one_for_one, intensity).intensity.to_h.values_at(:restarts, :window)
      end
      forger = forge(Intensity, 'Intensity')
      forger.define_method(:instance_of?) { |_| true }
      forger.define_method(:class) { Intensity }

      assert_raises(TypeError) { Supervisor.new(:one_for_one, forger.new(restarts: 1, window: 60)) }
    ensure
      Intensity.alias_method(:instance_of?, :__real_instance_of?)
      Intensity.remove_method(:__real_instance_of?)
    end

    # A value is frozen, so a singleton class can be opened but not given
    # methods. The tree keeps a fresh value rebuilt from the struct's own
    # fields, never the one it was handed.
    def test_the_supervisor_keeps_a_fresh_value_for_an_opened_singleton
      intensity = Intensity.new(2, 60).tap(&:singleton_class)

      assert_raises(FrozenError) { intensity.define_singleton_method(:restarts) { 0 } }
      kept = Supervisor.new(:one_for_one, intensity).intensity

      assert_equal Intensity.new(2, 60), kept
      refute_same intensity, kept
      assert_empty kept.singleton_methods
    end

    # A Marshal-edited value past MAX_RESTARTS is refused from the field the
    # binding read, with the same text `Decider.new` gives.
    def test_a_marshal_edited_restart_count_is_refused_as_the_decider_refuses_it
      dumped = Marshal.dump(Intensity.new(2, 60))
      intensity = Marshal.load(dumped.sub("i\x07".b, "i#{(Decider::MAX_RESTARTS + 6).chr}".b))

      assert_equal Decider::MAX_RESTARTS + 1, intensity.restarts
      expected = assert_raises(RangeError) { Decider.new(1, :one_for_one, intensity) }
      error = assert_raises(RangeError) { Supervisor.new(:one_for_one, intensity) }
      assert_equal expected.message, error.message
    end

    def test_the_supervisor_names_a_wrong_type_as_the_decider_does
      [[1, MINUTE], Span.new(1, 2), nil, [1, MINUTE].tap(&:singleton_class)].each do |wrong|
        expected = assert_raises(TypeError) { Decider.new(1, :one_for_one, wrong) }
        error = assert_raises(TypeError) { Supervisor.new(:one_for_one, wrong) }
        assert_equal expected.message, error.message
      end
      error = assert_raises(TypeError) { Supervisor.new(:one_for_one, [1, MINUTE]) }
      assert_equal 'expected a SupervisorMachines::Intensity, not Array', error.message
    end

    def test_the_validating_hooks_are_private
      %i[__intensity __span __restart __policy __strategy __supervised_intensity].each do |hook|
        assert_raises(NoMethodError) { SupervisorMachines.public_send(hook, nil) }
      end
    end

    def test_classes_held_by_the_binding_survive_gc_compaction
      decider = Decider.new(1, :one_for_one, Intensity.new(3, 60_000_000_000))
      # Compacting moves every unpinned object: a class the binding holds
      # unpinned would be a dangling reference and the next build would crash.
      GC.verify_compaction_references(expand_heap: true, toward: :empty)

      assert_equal Intensity.new(3, 60_000_000_000), decider.intensity
    end

    private

    # Runs the block with every listed reader raising if called.
    def redefined(readers)
      readers.each do |klass, names|
        names.each do |name|
          klass.alias_method(:"__real_#{name}", name)
          klass.define_method(name) { raise "#{name} reader called" }
        end
      end
      yield
    ensure
      readers.each do |klass, names|
        names.each do |name|
          klass.alias_method(name, :"__real_#{name}")
          klass.remove_method(:"__real_#{name}")
        end
      end
    end
  end
end
