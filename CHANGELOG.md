# Changelog

## [0.1.2](https://github.com/seuros/supervisor_machines/compare/supervisor_machines/v0.1.1...supervisor_machines/v0.1.2) (2026-11-01)


### Features

* back off granted restarts by recent restart count ([e7532e8](https://github.com/seuros/supervisor_machines/commit/e7532e8e98e43171c8d76f176161fdea50403a27))


### Bug Fixes

* a republished platform gem is skipped, not fatal ([39160e7](https://github.com/seuros/supervisor_machines/commit/39160e7e295d58500d846fb56d190eff6e803151))

## [0.1.1](https://github.com/seuros/supervisor_machines/compare/supervisor_machines-v0.1.0...supervisor_machines/v0.1.1) (2026-11-01)


### Features

* OTP-style supervision decisions ([013b054](https://github.com/seuros/supervisor_machines/commit/013b0543abb42eff100032c286642e9c26f5a907))
* pass a rubygems OTP through the environment ([d6b00e6](https://github.com/seuros/supervisor_machines/commit/d6b00e6f10606b4d8584d4f58999a184d2f749df))
* supervise threads over the decision core ([c423f63](https://github.com/seuros/supervisor_machines/commit/c423f63ceef340edfaef7eb2774eb353046b7556))


### Bug Fixes

* a source install can build, and a failed gem push fails the release ([5907461](https://github.com/seuros/supervisor_machines/commit/5907461674b3fc53bcbc6c077b39d6b61d959eaf))
* a started tree takes on no more children, and an unstarted one has no restarts ([49baae8](https://github.com/seuros/supervisor_machines/commit/49baae849adedfea8fc96ab18c8989e77e57edf7))
* a stop no longer waits out the restart backoff ([5de0f95](https://github.com/seuros/supervisor_machines/commit/5de0f95db1f15ceb2a4eb8fb8114d7ee48a5da8b))
* a stopped tree does not keep announcing its shutdown ([44f9e78](https://github.com/seuros/supervisor_machines/commit/44f9e78f8c5279be42fb29b10ef40ef8778b099b))
* an in-family floor, not a pessimistic pin ([7113e08](https://github.com/seuros/supervisor_machines/commit/7113e087b649320fe1d0b1dba1c2dabb268be175))
* bound the child index ([3761aab](https://github.com/seuros/supervisor_machines/commit/3761aab987111b4570aeaefc171175552d0c7948))
* call starters and the backoff outside the supervisor's lock ([b656111](https://github.com/seuros/supervisor_machines/commit/b656111a32fe5623a44b1dd7f665d708839b17b5))
* close six supervision lifecycle holes ([50d330f](https://github.com/seuros/supervisor_machines/commit/50d330f8bac893b75b8616b284bd0e1e1b006e99))
* collect the built gems whatever depth the download puts them at ([961776d](https://github.com/seuros/supervisor_machines/commit/961776db5edbee33b2fbe87494e6c6ac7ee0d3f7))
* give the tree lifecycle states so a failed start leaves nothing running ([dc7deba](https://github.com/seuros/supervisor_machines/commit/dc7debaff42f2440c80c6f43f057660c0a5c79a0))
* refuse a fractional restart intensity instead of tightening it quietly ([ea34650](https://github.com/seuros/supervisor_machines/commit/ea34650c0480f12e824ec488daa911daeda30ccf))
* ship the core's test files so the packaged crate builds ([4af208e](https://github.com/seuros/supervisor_machines/commit/4af208efd80a9e48275aedfe8e2137155acd59a0))
* the release tag carries the gem name, and the gate uses the aggregate output ([49982d6](https://github.com/seuros/supervisor_machines/commit/49982d60eed580789d6d52502c0f275576139df3))
* the tree reads the clock before taking its own lock ([6dd61ef](https://github.com/seuros/supervisor_machines/commit/6dd61efced111ae04b01ffdd7dfb4b559b0fd26e))
