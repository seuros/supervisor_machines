# frozen_string_literal: true

source 'https://rubygems.org'

gemspec

# deadline_machines is not released yet; use the sibling checkout when there is
# one, and the published gem once there is one of those.
sibling = File.expand_path('../deadline_machines', __dir__)
gem 'deadline_machines', path: sibling if File.directory?(sibling)

gem 'minitest', '~> 6.0'
gem 'rake', '~> 13.0'
gem 'rake-compiler', '~> 1.3'

platforms :mri do
  gem 'padded_cell', '~> 0.2'
  gem 'rbs', '~> 3.0'
end
