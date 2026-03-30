# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog:
<https://keepachangelog.com/en/1.1.0/>

This project follows Semantic Versioning:
<https://semver.org/>

## [1.12.0](https://github.com/dvalfrid/rigstats/compare/v1.11.0...v1.12.0) (2026-03-30)


### Features

* retry WMI hardware detection after startup if fields are missing ([72392ba](https://github.com/dvalfrid/rigstats/commit/72392ba49b8d05c14eb18aee7666fb9e044246f9))
* show read and write separately in storage sparkline ([f17140c](https://github.com/dvalfrid/rigstats/commit/f17140cbab2dcdce96d36bfd542a5d767ae4df1a))


### Bug Fixes

* show both upload and download in network sparkline with correct colours ([181c06b](https://github.com/dvalfrid/rigstats/commit/181c06bc5dfb4e44d76f47b2d27186cfa149bf97))
* truncate long rig name and reject version strings as model name ([edcea41](https://github.com/dvalfrid/rigstats/commit/edcea41878997386893525b6eb24ca224467753b))
* update spark tests to reflect renamed history series ([6a33251](https://github.com/dvalfrid/rigstats/commit/6a33251e538257db82c8454a76c520e70237e0ee))

## [1.11.0](https://github.com/dvalfrid/rigstats/compare/v1.10.1...v1.11.0) (2026-03-27)


### Features

* add Motherboard panel with fans, temps, and voltage rails ([819176e](https://github.com/dvalfrid/rigstats/commit/819176ecffec83945b05a0e28bebdd7fa695eba6))
* redesign dialogs with Windows 11 dark-mode aesthetic ([9a1aa2a](https://github.com/dvalfrid/rigstats/commit/9a1aa2a062bbe666e85bc5103dac940c019f8b21))

## [1.10.1](https://github.com/dvalfrid/rigstats/compare/v1.10.0...v1.10.1) (2026-03-26)


### Bug Fixes

* stop LHM before file extraction to prevent locked DLL errors on update ([a6ca37e](https://github.com/dvalfrid/rigstats/commit/a6ca37e9a7366398de26182245d272d832025c40))

## [1.10.0](https://github.com/dvalfrid/rigstats/compare/v1.9.3...v1.10.0) (2026-03-26)


### Features

* cycle disk drives in pages of three when more than three are present ([f1c55d5](https://github.com/dvalfrid/rigstats/commit/f1c55d5181f1545daf6e9758301ccf58f49c8459))


### Bug Fixes

* GPU sensors, SATA SSD temps, and DDR5 RAM type detection ([9c4b4de](https://github.com/dvalfrid/rigstats/commit/9c4b4de3996571154ba29dda9a92dc1725e4c502))
* sum all disk throughput, LPDDR types, VRAM fallback as Option ([a8028f0](https://github.com/dvalfrid/rigstats/commit/a8028f07399b63d6cc82ba63887928fe4bafbf41))

## [1.9.3](https://github.com/dvalfrid/rigstats/compare/v1.9.2...v1.9.3) (2026-03-25)


### Bug Fixes

* correct CPU temp and power sensor matching for Intel CPUs ([e53ae79](https://github.com/dvalfrid/rigstats/commit/e53ae797e2ad27d50464bb29251d7e181a874380))
* remove set_fullscreen(false) from set_main_height to prevent window shift on save ([10d602e](https://github.com/dvalfrid/rigstats/commit/10d602ed1f737d1562b9f7f98cb8a884534399d8))
* stable window placement and correct CPU sensor parsing ([1501395](https://github.com/dvalfrid/rigstats/commit/1501395dfd726fbfb4c2ff1d0fb2036c2c0812ab))

## [1.9.2](https://github.com/dvalfrid/rigstats/compare/v1.9.1...v1.9.2) (2026-03-25)


### Bug Fixes

* FHD Sidebar profile, GPU dGPU fix, panel-hide window resize ([2b72c27](https://github.com/dvalfrid/rigstats/commit/2b72c2792ee8ff60bf1501ea1b7562428e8b6b20))

## [1.9.1](https://github.com/dvalfrid/rigstats/compare/v1.9.0...v1.9.1) (2026-03-24)


### Bug Fixes

* repair three update-flow bugs ([b2a6b6e](https://github.com/dvalfrid/rigstats/commit/b2a6b6eb3209fc494b1c1d1cad00388dead070d4))

## [1.9.0](https://github.com/dvalfrid/rigstats/compare/v1.8.1...v1.9.0) (2026-03-24)


### Features

* add DDR5/DDR4 DIMM temperature to RAM panel ([4d65740](https://github.com/dvalfrid/rigstats/commit/4d657401a57272814ba4c486fc89021606954e96))
* temperature threshold alerts with configurable thresholds and notifications ([6cac71e](https://github.com/dvalfrid/rigstats/commit/6cac71e1ac6a63b44a572781ccf1e3fd9dcaf327))


### Bug Fixes

* show disk temp when WMI model map is empty; expand diagnostics ([61dbf3f](https://github.com/dvalfrid/rigstats/commit/61dbf3f76c0392cb5c98cc89470b42b108f241f9))

## [1.8.1](https://github.com/dvalfrid/rigstats/compare/v1.8.0...v1.8.1) (2026-03-24)


### Bug Fixes

* repair updater — missing event permission blocked install ([d96795f](https://github.com/dvalfrid/rigstats/commit/d96795f05b0b65f71c6262d71a2ec2b1314cb44a))

## [1.8.0](https://github.com/dvalfrid/rigstats/compare/v1.7.1...v1.8.0) (2026-03-24)


### Features

* add NVMe/SSD temperature display to disk panel ([8ab3187](https://github.com/dvalfrid/rigstats/commit/8ab3187ca2e264901ecf5fa580b66aa2678f6934))


### Bug Fixes

* write install.log to ProgramData instead of AppData ([eb968ac](https://github.com/dvalfrid/rigstats/commit/eb968acc80d1698ea9e8b03926c89004a40206fd))

## [1.7.1](https://github.com/dvalfrid/rigstats/compare/v1.7.0...v1.7.1) (2026-03-23)


### Bug Fixes

* **updater:** show full changelog history and polish no-update view ([b6ed4dd](https://github.com/dvalfrid/rigstats/commit/b6ed4dd1091e56a6b563c6d1d96fd0a853c74306))

## [1.7.0](https://github.com/dvalfrid/rigstats/compare/v1.6.0...v1.7.0) (2026-03-23)


### Features

* **updater:** move changelog to updater dialog and refine UX ([b14739c](https://github.com/dvalfrid/rigstats/commit/b14739c331a9b81790b94d540ad168cbe16297a4))

## [1.6.0](https://github.com/dvalfrid/rigstats/compare/v1.5.1...v1.6.0) (2026-03-23)


### Features

* **updater:** add auto-update with background check and progress UI ([3114d2d](https://github.com/dvalfrid/rigstats/commit/3114d2d8949f54bb71b52797d0ca61689d032c50))


### Bug Fixes

* remove window decorations and force Node 24 in CI ([e2cb4fd](https://github.com/dvalfrid/rigstats/commit/e2cb4fd60e775bfd91dcdf7042f69619248980ee))

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
