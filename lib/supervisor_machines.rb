# frozen_string_literal: true

require 'chrono_machines'
require 'deadline_machines'

require_relative 'supervisor_machines/version'
require_relative 'supervisor_machines/values'

# Decider and the validating hooks the values above call are defined by the
# native extension, which binds the `supervisor-machines` crate; there is no
# pure-Ruby implementation to fall back to.
require 'supervisor_machines_native/supervisor_machines_native'

module SupervisorMachines
  private_class_method :__intensity, :__span, :__restart, :__policy, :__strategy, :__supervised_intensity

  # Decider wraps native state and has no allocator: a subclass's `new`
  # would build the base class anyway, so subclassing raises too.
  Decider.define_singleton_method(:inherited) { |_| raise TypeError, "#{Decider} cannot be subclassed" }
end

require_relative 'supervisor_machines/errors'
require_relative 'supervisor_machines/supervisor'
