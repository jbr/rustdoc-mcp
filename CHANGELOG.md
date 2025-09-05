# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.4](https://github.com/jbr/rustdoc-mcp/compare/v0.1.3...v0.1.4) - 2025-09-05

### Added

- upgrade nightly version
- *(workspaces)* show which dependencies are used by which workspace crates
- *(workspaces)* list_crates accepts a workspace_member argument
- *(workspaces)* scoping by setting working directory within a subcrate
- *(workspaces)* initial workspace support
- add search

### Other

- Merge pull request #9 from jbr/release-plz-2025-07-26T19-03-36Z
- add search to readme

## [0.1.3](https://github.com/jbr/rustdoc-mcp/compare/v0.1.2...v0.1.3) - 2025-07-26

### Added

- add versions and dev dep indicators to list-crates

### Other

- change log level in ci and add .claude to gitignore
- improve user-facing docs

## [0.1.2](https://github.com/jbr/rustdoc-mcp/compare/v0.1.1...v0.1.2) - 2025-07-26

### Added

- simplify and improve trait impl logic

### Other

- add a test for list-crates
- split up formatting.rs
- fix readme json
- remove a copypasta reference to fs-mcp
- remove unused trait impls (dead code)

## [0.1.1](https://github.com/jbr/rustdoc-mcp/compare/v0.1.0...v0.1.1) - 2025-07-25

### Other

- add badges to readme
- run coverage on nightly
- pub -> pub(crate) wherever possible
- better test isolation
- add rust-docs-json
- add missing snap
- rename snapshots
- use trusted publisher workflow instead of a token
