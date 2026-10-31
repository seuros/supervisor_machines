# frozen_string_literal: true

require 'chrono_machines'
require 'deadline_machines'

require_relative 'supervisor_machines/version'

# Decider is defined by the native extension, which binds the
# `supervisor-machines` crate; there is no pure-Ruby implementation to fall
# back to.
require 'supervisor_machines_native/supervisor_machines_native'

require_relative 'supervisor_machines/errors'
require_relative 'supervisor_machines/supervisor'
