# Changelog

## [0.7.0](https://github.com/seuros/supervisor_machines/compare/supervisor-machines-v0.6.0...supervisor-machines-v0.7.0) (2026-11-16)


### ⚠ BREAKING CHANGES

* take chrono_machines shapes for the respawn delay and seal Decider

### Features

* take chrono_machines shapes for the respawn delay and seal Decider ([b837f98](https://github.com/seuros/supervisor_machines/commit/b837f981e1e8eba163a54bafed04b94a18b7cd0f))

## [0.6.0](https://github.com/seuros/supervisor_machines/compare/supervisor-machines-v0.5.0...supervisor-machines-v0.6.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* adopt matryona-machines 0.2: struct-only reads, native Restart check, symbols! tables, caret rb-sys

### Features

* adopt matryona-machines 0.2: struct-only reads, native Restart check, symbols! tables, caret rb-sys ([836afc6](https://github.com/seuros/supervisor_machines/commit/836afc6d615cf53a61e57a75d88b4c14a3ea4329))

## [0.5.0](https://github.com/seuros/supervisor_machines/compare/supervisor-machines-v0.4.0...supervisor-machines-v0.5.0) (2026-11-14)


### ⚠ BREAKING CHANGES

* refuse unknown children, Duration time, multiset restore and absorb, exact-class boundary, Decider core

### Features

* refuse unknown children, Duration time, multiset restore and absorb, exact-class boundary, Decider core ([16f739b](https://github.com/seuros/supervisor_machines/commit/16f739b821052bb676b4086ec63927d9cc0ee8e9))

## [0.4.0](https://github.com/seuros/supervisor_machines/compare/supervisor-machines-v0.3.0...supervisor-machines-v0.4.0) (2026-11-06)


### Features

* share one restart history across processes ([b08b7e9](https://github.com/seuros/supervisor_machines/commit/b08b7e90353b97ef83af5ff0e77b8b68cec41431))


### Bug Fixes

* keep test modules out of the packaged crate ([746649b](https://github.com/seuros/supervisor_machines/commit/746649b776f509d03ba770e91691d8b32e7c0a57))

## [0.3.0](https://github.com/seuros/supervisor_machines/compare/supervisor-machines-v0.2.0...supervisor-machines-v0.3.0) (2026-11-01)


### Features

* back off granted restarts by recent restart count ([e7532e8](https://github.com/seuros/supervisor_machines/commit/e7532e8e98e43171c8d76f176161fdea50403a27))

## [0.2.0](https://github.com/seuros/supervisor_machines/compare/supervisor-machines-v0.1.0...supervisor-machines-v0.2.0) (2026-11-01)


### Features

* OTP-style supervision decisions ([013b054](https://github.com/seuros/supervisor_machines/commit/013b0543abb42eff100032c286642e9c26f5a907))
* supervise threads over the decision core ([c423f63](https://github.com/seuros/supervisor_machines/commit/c423f63ceef340edfaef7eb2774eb353046b7556))


### Bug Fixes

* ship the core's test files so the packaged crate builds ([4af208e](https://github.com/seuros/supervisor_machines/commit/4af208efd80a9e48275aedfe8e2137155acd59a0))
