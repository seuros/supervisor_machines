# frozen_string_literal: true

source 'https://rubygems.org'

gemspec

# Use a sibling checkout when there is one, so a change there is tested here
# before it is published, and the published gem otherwise.
%w[deadline_machines chrono_machines].each do |name|
  sibling = File.expand_path("../#{name}", __dir__)
  gem name, path: sibling if File.directory?(sibling)
end

gem 'minitest', '~> 6.0'
gem 'rake', '~> 13.0'
gem 'rake-compiler', '~> 1.3'

platforms :mri do
  gem 'padded_cell', '~> 0.2'
  gem 'rbs', '~> 3.0'
end
