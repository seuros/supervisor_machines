# frozen_string_literal: true

require_relative 'test_helper'
require 'open3'
require 'rbconfig'

# The class of the value the extension wraps (Decider) is fixed when it
# loads. Nothing a caller does later, redefining the constant or
# `Object.const_get`, reaches a Decider being built. Each case runs in a
# fresh process: the class is fixed once per process, at load.
class ClassLookupTest < Minitest::Test
  LIB = File.expand_path('../lib', __dir__)

  PRELUDE = <<~RUBY
    require 'supervisor_machines'
    S = SupervisorMachines
    intensity = S::Intensity.new(3, 5_000_000_000)
  RUBY

  # Runs `body` after PRELUDE in a new Ruby; answers its last line of output.
  def isolated(body)
    out, err, status = Open3.capture3(RbConfig.ruby, '-I', LIB, '-e', PRELUDE + body)
    assert status.success?, "exited #{status.exitstatus}: #{err}"
    out.lines.last&.chomp
  end

  # Bug: a Decider constant redefined before the first Decider was built made
  # `new` and `restore` answer instances of the new class, with none of the
  # Decider's methods.
  def test_a_redefined_decider_constant_does_not_change_the_decider_class
    assert_equal 'true true 1', isolated(<<~RUBY)
      original = S::Decider
      S.send(:remove_const, :Decider)
      S.const_set(:Decider, Class.new)
      built = original.new(1, :one_for_one, intensity)
      restored = original.restore(1, :one_for_one, intensity, [5], 10)
      puts [built.instance_of?(original), restored.instance_of?(original), built.children].join(' ')
    RUBY
  end

  # Bug: building the first Decider ran `Object.const_get`, a caller's
  # Ruby, inside the extension.
  def test_building_a_decider_runs_no_const_get
    assert_equal '0 Decider', isolated(<<~RUBY)
      hits = 0
      Object.define_singleton_method(:const_get) do |*args|
        hits += 1
        super(*args)
      end
      decider = S::Decider.new(1, :one_for_one, intensity)
      puts [hits, decider.class.name.split('::').last].join(' ')
    RUBY
  end

  # Bug: a redefined `Object.const_get` that raised hit an `unwrap` in the
  # extension: an uncatchable fatal, and the process exited.
  def test_a_raising_const_get_does_not_kill_the_process
    assert_equal '1 1', isolated(<<~RUBY)
      Object.define_singleton_method(:const_get) { |*| raise IOError, 'lookup ran' }
      built = S::Decider.new(1, :one_for_one, intensity)
      restored = S::Decider.restore(1, :one_for_one, intensity, [5], 10)
      puts [built.children, restored.children].join(' ')
    RUBY
  end
end
