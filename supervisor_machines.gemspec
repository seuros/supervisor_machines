# frozen_string_literal: true

require_relative 'lib/supervisor_machines/version'

Gem::Specification.new do |spec|
  spec.name = 'supervisor_machines'
  spec.version = SupervisorMachines::VERSION
  spec.authors = ['Abdelkader Boudih']
  spec.email = ['oss@seuros.com']

  spec.summary = 'OTP-style supervision trees over Ruby threads'
  spec.description = 'A supervision tree that restarts what died the way OTP would: one_for_one, one_for_all or ' \
                     'rest_for_one, bounded by a restart intensity that escalates instead of thrashing. The ' \
                     'decisions come from the no_std supervisor-machines crate; Ruby owns the threads, the ' \
                     'restart backoff comes from chrono_machines and the shutdown budget from deadline_machines.'
  spec.homepage = 'https://github.com/seuros/supervisor_machines'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 4.0.0'

  spec.metadata['source_code_uri'] = spec.homepage
  spec.metadata['changelog_uri'] = "#{spec.homepage}/blob/main/CHANGELOG.md"
  spec.metadata['bug_tracker_uri'] = "#{spec.homepage}/issues"
  spec.metadata['rubygems_mfa_required'] = 'true'
  spec.metadata['cargo_crate_name'] = 'supervisor_machines_native'
  spec.metadata['cargo_manifest_path'] = 'ext/supervisor_machines_native/ffi/Cargo.toml'

  spec.files = Dir.glob(%w[
                          lib/**/*.rb
                          sig/**/*.rbs
                          ext/**/*.{rb,rs,toml}
                          Cargo.toml
                          Cargo.lock
                          README.md
                          CHANGELOG.md
                          LICENSE.txt
                        ]).select { |path| File.exist?(path) }
                   .reject { |path| path =~ %r{/tests(/|\.rs\z)} }
  spec.require_paths = ['lib']
  spec.extensions = ['ext/supervisor_machines_native/extconf.rb']

  spec.add_dependency 'chrono_machines', '>= 0.12.1'
  spec.add_dependency 'deadline_machines', '>= 0.1'
  # A source install runs extconf.rb, which requires rb_sys/mkmf, and RubyGems
  # clears GEM_PATH while building the extension. Without this the build only
  # works where rb_sys happens to be installed globally.
  spec.add_dependency 'rb_sys', '~> 0.9'
end
