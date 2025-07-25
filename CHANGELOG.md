# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/jbr/rustdoc-mcp/releases/tag/v0.1.0) - 2025-07-25

### Added

- [**breaking**] rewrite rustdoc-json-mcp!
- *(get_item_details)* enhance with detailed and include_impls parameters
- implement project-based architecture with scoped rebuilds
- implement basic rustdoc JSON MCP server

### Fixed

- fmt and clippy

### Other

- clippy
- *(server)* extract match arms from handle_get_item_details into helper functions
- code tidying and organization improvements
- integrate crate listing into set_project tool
