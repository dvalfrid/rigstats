# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog:
https://keepachangelog.com/en/1.1.0/

This project follows Semantic Versioning:
https://semver.org/

## [Unreleased]

### Added
- 

### Changed
- 

### Fixed
- 

### Removed
- 

## [1.0.0] - 2026-03-14

### Added
- Automated CI workflows for verify, build, and release.
- Automated LibreHardwareMonitor preparation with pinned version download.
- Resource bundling for LibreHardwareMonitor binaries and default config.
- Frontend unit tests and backend unit tests.

### Changed
- Build and verify flows now prepare LibreHardwareMonitor before Rust checks.
- Documentation split into focused guides under docs.

### Fixed
- CI failure caused by missing vendor/lhm during Rust build script execution.

[Unreleased]: https://github.com/codeby/rigstats/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/codeby/rigstats/releases/tag/v1.0.0
