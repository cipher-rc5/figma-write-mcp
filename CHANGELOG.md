# Changelog

All notable changes to this project are documented here. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `LICENSE` (MIT), `SECURITY.md`, `CONTRIBUTING.md`.
- Continuous-integration workflow (`.github/workflows/ci.yml`) running
  `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and the
  plugin's `bun run typecheck` and `bun test`.
- Release workflow (`.github/workflows/release.yml`) producing per-OS
  binaries with SHA256 checksums on tagged commits.
- Shared-secret handshake on the bridge: server generates and stores a
  random secret on first launch; the plugin must echo it as the first
  message of every connection before any tool call is honoured.
- `protocol_version` field on the request/response envelope.
- Structured error pass-through: tool failures carry a JSON
  `{code, message}` body so MCP clients can branch on the failure kind.
- `update_node_properties` now returns an `ignored` map alongside `applied`
  so callers can distinguish "applied" from "silently dropped".
- Rust unit tests for `parse_response`, schema construction, secret loading,
  and pending-map cleanup.
- TypeScript unit tests for `parseReq`, `parseBridgeMessage`, `hexToRgb`,
  `isValidHex`, and the numeric validators.
- Property-style randomised tests for the request parser.
- `bun run build` now bundles `code.ts` into `dist/code.js` via `bun build`.

### Changed

- `Bridge::call` now removes the pending-map entry on timeout and on
  disconnect (no more silent memory growth).
- On plugin disconnect, the server proactively errors every in-flight
  caller instead of letting them wait the full 30 s timeout.
- `unbounded_channel` replaced with a bounded `mpsc::channel(64)` to add
  backpressure on a stuck plugin.
- `accept_loop` bind failure is now fatal: the server exits non-zero with
  a clear error instead of staying alive with a useless bridge.
- `main()` installs a `tokio::signal::ctrl_c` handler so SIGINT/SIGTERM
  shuts the listener and bridge down cleanly.
- All four MCP tool schemas now declare `additionalProperties: false`.
- `FigmaServer::tools()` is memoised in a `OnceLock<Vec<Tool>>`.
- `tokio = { features = ["full"] }` narrowed to
  `["net","sync","time","rt-multi-thread","macros","signal","io-util","fs"]`.
- `Cargo.toml` now ships `description`, `license`, `repository`,
  `homepage`, and `readme` metadata.
- Plugin's `hexToRgb` rejects invalid input with `invalid_params`.
- Plugin validates `font_size`, `width`, `line_height_pct`, `x`, and `y`
  as finite numbers before passing them to the Figma API.
- `handleUpdateNodeProperties` returns `wrong_node_type` (in the `ignored`
  map) when a key targets a mixin the node does not implement.
- Plugin guards `getRangeAllFontNames` against empty text content.
- The `is*` predicate helpers are co-located with the type declarations at
  the top of `code.ts`.

### Fixed

- Plugin no longer drops malformed bridge frames silently; the UI log
  panel now records each drop with the underlying parse error.
- Static schema parsing in the server uses `?` propagation; no more
  `.expect(...)` panic path.

### Security

- Authenticated handshake on the loopback bridge (see `SECURITY.md`).

## [0.1.0] - 2026-05-19

### Added

- Initial release.
- MCP server with four tools: `set_text`, `delete_node`, `create_text_node`,
  `update_node_properties`.
- Figma plugin scaffold with WebSocket bridge to the MCP server.
- Manual smoke test (`SMOKE_TEST.md`).
