# Improvements Checklist

**Generated from review:** _dev/reviews/001/critical_analysis.md
**Date:** 2026-05-21

---

## P0 — Blockers

- [ ] **[Testing]** Add Rust unit tests for `Bridge::call` correlation, timeout cleanup, and disconnect handling — `server/src/main.rs:60-97` — Effort: M
- [ ] **[Testing]** Add plugin tests for `parseReq`, `parseBridgeMessage`, `hexToRgb`, and each handler's input validation — `plugin/code.ts:246-269` — Effort: M
- [ ] **[CI/CD]** Add `.github/workflows/ci.yml` running `cargo check`, `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `bun install --frozen-lockfile`, `bun run typecheck` on push/PR — Effort: S
- [ ] **[Docs]** Add `LICENSE` (MIT or Apache-2.0); reference it from `Cargo.toml:license =` and `package.json:license` — Effort: S
- [ ] **[Safety]** Make `accept_loop` bind failure fatal (return `Err` to `main` or exit non-zero) so the MCP server fails fast instead of silently reporting "plugin not connected" — `server/src/main.rs:100-106` — Effort: S

## P1 — Pre-release

- [ ] **[Concurrency]** On plugin disconnect, drain `self.pending` and notify all waiting callers with a structured "plugin disconnected" error instead of letting them wait for the 30s timeout — `server/src/main.rs:154-160` — Effort: M
- [ ] **[Safety]** On `Bridge::call` timeout, remove the entry from `self.pending` to prevent unbounded HashMap growth — `server/src/main.rs:83-86` — Effort: S
- [ ] **[Safety]** Validate `fill_hex` against `^#?[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$` in `create_text_node`; return `invalid_params` on mismatch — `plugin/code.ts:70-74, 141-144` — Effort: S
- [ ] **[Safety]** Validate numeric params (`font_size > 0`, `width > 0`, `line_height_pct > 0`, finite x/y) before passing to Figma API; return `invalid_params` on bad input — `plugin/code.ts:131-156` — Effort: S
- [ ] **[Error Handling]** Preserve structured `{code, message}` plugin errors all the way through `Bridge::call` and `call_tool` so MCP clients can branch on `node_not_found` vs `font_not_loaded` — `server/src/main.rs:88-96, 286-308` — Effort: M
- [ ] **[Error Handling]** Distinguish `wrong_node_type` from `internal` in `handleUpdateNodeProperties`: if user passes `rotation` on a non-`LayoutMixin` node, return `wrong_node_type` rather than silently dropping or generically failing — `plugin/code.ts:172-217` — Effort: M
- [ ] **[API Design]** Surface unapplied keys in `update_node_properties` response (e.g. `applied` plus `ignored: { rotation: "wrong_node_type" }`) so callers don't have to diff — `plugin/code.ts:172-217` — Effort: M
- [ ] **[Security]** Add a shared-secret handshake on first WS frame so a non-plugin local process cannot impersonate the plugin; persist the secret in `~/Library/Application Support/figma-write-mcp/secret` and inject into the plugin via `manifest.json`-side env or first-launch UI prompt — `server/src/main.rs:99-160`, `plugin/ui.html:29` — Effort: L
- [ ] **[Docs]** Add `SECURITY.md` with disclosure address and reiterate the auth model from `README.md:101-103` — Effort: S
- [ ] **[Docs]** Add `CHANGELOG.md` and adopt Keep-a-Changelog format; pre-populate with `0.1.0` — Effort: S
- [ ] **[CI/CD]** Add a release workflow that produces SHA256 checksums for `figma-write-mcp` per-OS and uploads to a tagged GitHub release — Effort: M

## P2 — Should-fix

- [ ] **[Concurrency]** Replace `unbounded_channel` with a bounded mpsc (e.g. capacity 64) for `outbound` to add backpressure on a stuck plugin — `server/src/main.rs:127` — Effort: S
- [ ] **[Concurrency]** Install a `tokio::signal::ctrl_c` handler in `main` that closes the listener and drops `outbound` so the process exits cleanly — `server/src/main.rs:311-329` — Effort: S
- [ ] **[API Design]** Add `additionalProperties: false` to all four tool schemas to catch typos — `server/src/main.rs:190-258` — Effort: S
- [ ] **[API Design]** Add a `protocol_version` field to the envelope and refuse mismatched versions; document in `PROTOCOL.md` — Effort: M
- [ ] **[Safety]** Guard `getRangeAllFontNames` against empty `characters` (skip if `tn.characters.length === 0`) — `plugin/code.ts:67` — Effort: S
- [ ] **[Error Handling]** Log dropped malformed bridge frames to the plugin Log panel so silent drops are observable — `plugin/code.ts:276-279`, `plugin/ui.html:39-43` — Effort: S
- [ ] **[Docs]** Clarify FigJam vs Figma operation matrix in `README.md` (`SECTION` is Figma-only; `TEXT` semantics differ) — `manifest.json:7-10` — Effort: S
- [ ] **[Docs]** Add `CONTRIBUTING.md` pointing at `justfile` recipes (`just fmt`, `just lint`, `just test`, `just typecheck`) — Effort: S
- [ ] **[Conventions]** Fill in `Cargo.toml` package metadata: `description`, `license`, `repository`, `homepage`, `readme` — `server/Cargo.toml:1-5` — Effort: S

## P3 — Nice-to-have

- [ ] **[Performance]** Cache `FigmaServer::tools()` in a `OnceLock<Vec<Tool>>` so list_tools/get_tool don't re-parse schemas per call — `server/src/main.rs:190-258` — Effort: S
- [ ] **[Dependencies]** Trim `tokio = { features = ["full"] }` to `["net","sync","time","rt-multi-thread","macros"]` — `server/Cargo.toml:12` — Effort: S
- [ ] **[Testing]** Add a fuzz target for `parseReq` (TypeScript) using a property-test library, and one for `PluginResponse` deserialization (Rust) using `cargo-fuzz` — Effort: L
- [ ] **[Safety]** Replace the static-schema `.expect(...)` at `server/src/main.rs:261` with `?`-propagation through a fallible `tools()` — Effort: S
- [ ] **[Conventions]** Move `is*` predicate helpers (`plugin/code.ts:159-170`) to the top of the file next to the type declarations for readability — Effort: S
- [ ] **[Performance]** Pre-allocate `applied` with `Object.create(null)` if churn matters; almost certainly N/A at this scale — Effort: S

---

## Progress

**Total items:** 31
**P0:** 5 | **P1:** 12 | **P2:** 9 | **P3:** 5
