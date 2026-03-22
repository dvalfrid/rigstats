# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog:
<https://keepachangelog.com/en/1.1.0/>

This project follows Semantic Versioning:
<https://semver.org/>

## [1.5.1](https://github.com/dvalfrid/rigstats/compare/v1.5.0...v1.5.1) (2026-03-22)


### Bug Fixes

* **lhm:** detect Intel CPU temperature via priority sensor list ([43c58b3](https://github.com/dvalfrid/rigstats/commit/43c58b3d981c381e77bb3600bebd431b05e6c46a))

## [1.5.0](https://github.com/dvalfrid/rigstats/compare/v1.4.0...v1.5.0) (2026-03-21)


### Features

* **diagnostics:** add installer log to diagnostics ZIP and pretty-print all JSON files ([04611e6](https://github.com/dvalfrid/rigstats/commit/04611e6bbfd187763172ff76fdf2d70e58a25bc0))


### Bug Fixes

* **hardware:** add PowerShell fallback for model name detection and filter placeholder values ([bf1b5b5](https://github.com/dvalfrid/rigstats/commit/bf1b5b55f516f4bba7f10a87ca8cd4b90b4e96a8))
* **lhm:** detect Intel CPU temperature via priority sensor list (Core (Tctl/Tdie) → CPU Package → Core Average)
* **settings:** auto-detect model name immediately when field is cleared ([3b3e4f8](https://github.com/dvalfrid/rigstats/commit/3b3e4f8e0d679c773d51f05a7675f4817d77a183))

## [1.4.0](https://github.com/dvalfrid/rigstats/compare/v1.3.1...v1.4.0) (2026-03-21)


### Features

* **autostart:** add launch-at-startup toggle to Settings ([c69057c](https://github.com/dvalfrid/rigstats/commit/c69057cf20402b5c8c1478004a83e28de1ef168c))
* **ui:** add drag-to-reorder panel ordering ([2fb5f92](https://github.com/dvalfrid/rigstats/commit/2fb5f92ea4ff6d2c9e89ffc6bbddb3ddcfe43339))


### Bug Fixes

* **ui:** add consistent hover animations to all dialog buttons and panel toggles ([d203fd0](https://github.com/dvalfrid/rigstats/commit/d203fd01ad5de4889f80f3e1db1431f309fc8d77))

## [1.3.1](https://github.com/dvalfrid/rigstats/compare/v1.3.0...v1.3.1) (2026-03-20)


### Bug Fixes

* **lhm:** fix scheduled task setup and improve diagnostics ([dc6c25c](https://github.com/dvalfrid/rigstats/commit/dc6c25c4247af0fcd18f66dd99889f27345d5606))

## [1.3.0](https://github.com/dvalfrid/rigstats/compare/v1.2.2...v1.3.0) (2026-03-20)


### Features

* **about:** add changelog viewer with version history ([a99f6e4](https://github.com/dvalfrid/rigstats/commit/a99f6e484d3caabf67b7cca51a0e4434d4c838bf))
* **diagnostics:** add display topology to diagnostics export ([801fb86](https://github.com/dvalfrid/rigstats/commit/801fb86295189b4ed0fea9584fc3709f988d93e0))
* rebrand to RIGStats and add SEO + custom domain support ([309ae48](https://github.com/dvalfrid/rigstats/commit/309ae48ee73f363fafeaf3ed144c4c9fc1e03e18))
* **website:** add product website with GitHub Pages deployment ([bc35a02](https://github.com/dvalfrid/rigstats/commit/bc35a021c70895ea7c75e4cac55b5d66d3c12b1c))

## [1.2.2](https://github.com/dvalfrid/rigstats/compare/v1.2.1...v1.2.2) (2026-03-20)


### Bug Fixes

* fill dialog shell to window height and pin buttons to bottom ([c6114b7](https://github.com/dvalfrid/rigstats/commit/c6114b7a8f229516dcd9e6e6e0b84a7ed5c4fbc2))

## [1.2.1](https://github.com/dvalfrid/rigstats/compare/v1.2.0...v1.2.1) (2026-03-20)


### Bug Fixes

* correct Cargo.toml version to 1.2.0 and add release-please marker ([337fbde](https://github.com/dvalfrid/rigstats/commit/337fbde6761610c8e64539cec4546a02457b53df))

## [1.2.0](https://github.com/dvalfrid/rigstats/compare/v1.1.0...v1.2.0) (2026-03-19)


### Features

* **display:** add profile-aware dashboard layouts with live size preview and monitor fallback improvements ([31ba3a2](https://github.com/dvalfrid/rigstats/commit/31ba3a22ceea8e05478bc8bc23c0b1234491792e))

## [1.1.0](https://github.com/dvalfrid/rigstats/compare/v1.0.1...v1.1.0) (2026-03-14)


### Features

* add Collect Diagnostics export to Status dialog ([745ef04](https://github.com/dvalfrid/rigstats/commit/745ef04aa1b526c05c3da7698f65731cb6dd59e7))
* add panel visibility control + live preview and unify dialogs to ultra-compact layout ([73323b1](https://github.com/dvalfrid/rigstats/commit/73323b13210190332e2106e020f0571c999acc8d))
* Add support for Acer, Alienware, Gigabyte, HP Omen, Lenovo, MSI, Razer ([7b0b99f](https://github.com/dvalfrid/rigstats/commit/7b0b99f0c0cb1bd5b5b72e57854d764b4947803f))


### Bug Fixes

* eliminate memory leaks from reqwest client churn and orphaned Tauri listeners ([0a7cbf7](https://github.com/dvalfrid/rigstats/commit/0a7cbf7c8e3a3ffe23e5fc65ad2c1651c061ef05))

## [1.0.1](https://github.com/dvalfrid/rigstats/compare/v1.0.0...v1.0.1) (2026-03-14)


### Bug Fixes

* **Fix installation and debugging:** Fix installation and Status page for debugging ([2efb9bb](https://github.com/dvalfrid/rigstats/commit/2efb9bb7fa17fa328ba74f3eb351d0d7e769d109))
* Remove support for msi and some UI fixes ([0e676b3](https://github.com/dvalfrid/rigstats/commit/0e676b36646190d6e9b5e3e49dbee2aee73c3db4))

## 1.0.0 (2026-03-14)


### Miscellaneous Chores

* bootstrap first release ([9b40fc4](https://github.com/dvalfrid/rigstats/commit/9b40fc4a5e893cd57d1d710a2dfb54877b9b99e8))
