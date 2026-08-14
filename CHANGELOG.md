# Changelog

## [2.5.0](https://github.com/Perdolique/worsier/compare/v2.4.1...v2.5.0) (2026-08-14)


### Features

* **formatter:** add type alias spacing ([9cc7f93](https://github.com/Perdolique/worsier/commit/9cc7f938a357875939b71bf3dcc6a576007c5ab4))

## [2.4.1](https://github.com/Perdolique/worsier/compare/v2.4.0...v2.4.1) (2026-08-14)


### Bug Fixes

* **release:** restore Windows x64 package ([e5caed6](https://github.com/Perdolique/worsier/commit/e5caed675c26085b46e06bed771fb2a9a542e669))

## [2.4.0](https://github.com/Perdolique/worsier/compare/v2.3.0...v2.4.0) (2026-08-14)


### Features

* **formatter:** add interface layout rule ([b81d7b0](https://github.com/Perdolique/worsier/commit/b81d7b0091ed16f94b762475b630efb36bd6712d))
* **formatter:** add interface layout rule ([863f5df](https://github.com/Perdolique/worsier/commit/863f5df6d2074f14e450d09c50886550db630271))

## [2.3.0](https://github.com/Perdolique/worsier/compare/v2.2.0...v2.3.0) (2026-08-13)


### Features

* **formatter:** add semicolon rules ([d3d586c](https://github.com/Perdolique/worsier/commit/d3d586c14cc500ec3a62e36e54ca426f1b1c01ff))

## [2.2.0](https://github.com/Perdolique/worsier/compare/v2.1.0...v2.2.0) (2026-08-13)


### Features

* **formatter:** add trailing comma formatting ([613a3eb](https://github.com/Perdolique/worsier/commit/613a3eb521c9df8ea60c191b797ae24cb48eea9c))

## [2.1.0](https://github.com/Perdolique/worsier/compare/v2.0.0...v2.1.0) (2026-08-13)


### Features

* **all:** make configuration optional ([2f2fe43](https://github.com/Perdolique/worsier/commit/2f2fe4356a8c6f6f367de2e20279d357315745ed))

## [2.0.0](https://github.com/Perdolique/worsier/compare/v1.1.0...v2.0.0) (2026-08-12)


### ⚠ BREAKING CHANGES

* **formatter:** rules.imports and rules.variables were removed. Use rules.importLayout and rules.statementSpacing instead.

### Features

* **formatter:** split layout and spacing ([252513f](https://github.com/Perdolique/worsier/commit/252513fbcd1a3b26c25d255a1b53c0e93ff41702))
* **formatter:** split layout and spacing rules ([0790671](https://github.com/Perdolique/worsier/commit/0790671963e6ce3d46a6ae7b327d4d58c5d840dd))


### Bug Fixes

* **formatter:** harden spacing rules ([b670106](https://github.com/Perdolique/worsier/commit/b670106aab82c3a5ca7b9ee0c064a5a202b4856f))

## [1.1.0](https://github.com/Perdolique/worsier/compare/v1.0.6...v1.1.0) (2026-08-12)


### Features

* **formatter:** format variable boundaries ([7a4634c](https://github.com/Perdolique/worsier/commit/7a4634cc0a54d91896267c0f0325a3e37f62f980))

## [1.0.6](https://github.com/Perdolique/worsier/compare/v1.0.5...v1.0.6) (2026-08-12)


### Bug Fixes

* **release:** require publishable platforms ([3a9c8a0](https://github.com/Perdolique/worsier/commit/3a9c8a04c48367b83d421fc015e0ab4143eafd1f))

## [1.0.5](https://github.com/Perdolique/worsier/compare/v1.0.4...v1.0.5) (2026-08-12)


### Bug Fixes

* **npm:** document Windows package gap ([b2e6339](https://github.com/Perdolique/worsier/commit/b2e633949bea864f64555b1caeceacc3b1e8d227))

## [1.0.4](https://github.com/Perdolique/worsier/compare/v1.0.3...v1.0.4) (2026-08-12)


### Bug Fixes

* **release:** trigger patch release ([e9c652e](https://github.com/Perdolique/worsier/commit/e9c652e8d81b74b0d156198c0aaa9eb3143c32d4))

## [1.0.3](https://github.com/Perdolique/worsier/compare/v1.0.2...v1.0.3) (2026-08-12)


### Bug Fixes

* **npm:** handle Windows tar line endings ([88e04f6](https://github.com/Perdolique/worsier/commit/88e04f6c7dda6a5c54e8f1b559a00497f8105875))

## [1.0.2](https://github.com/Perdolique/worsier/compare/v1.0.1...v1.0.2) (2026-08-12)


### Bug Fixes

* **all:** harden formatter and release flow ([50063f1](https://github.com/Perdolique/worsier/commit/50063f1a4872e5f226c57582916e1d46b02921da))
* harden formatter and release pipeline ([c9541bc](https://github.com/Perdolique/worsier/commit/c9541bc38e35a4a9928165bc21e99471aa078e7b))

## [1.0.1](https://github.com/Perdolique/worsier/compare/v1.0.0...v1.0.1) (2026-08-12)


### Bug Fixes

* **release:** scope smoke tests to npm ([0bcc48a](https://github.com/Perdolique/worsier/commit/0bcc48a145163a1748b1ea46effa2609e7ac2a9b))

## [1.0.0](https://github.com/Perdolique/worsier/compare/v0.1.0...v1.0.0) (2026-08-12)


### ⚠ BREAKING CHANGES

* **formatter:** Worsier now formats only static imports. Legacy style configuration keys were removed; use lineWidth, verifyAst, rules.imports, and ignorePatterns.

### Code Refactoring

* **formatter:** format imports only ([5fb90b0](https://github.com/Perdolique/worsier/commit/5fb90b0d55d67b41a5477256201d43cf2684a309))

## 0.1.0 (2026-08-11)


### Features

* **all:** implement Worsier formatter ([2b62980](https://github.com/Perdolique/worsier/commit/2b62980cf266be02c9ed50e5e4a17efe0bd2d701))
