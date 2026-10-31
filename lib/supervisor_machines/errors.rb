# frozen_string_literal: true

module SupervisorMachines
  class Error < StandardError; end

  # Raised once a supervisor has spent its restart intensity. Its children are
  # down and the decision now belongs to whatever supervises the supervisor.
  class Escalated < Error; end
end
