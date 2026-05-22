# Critical Analysis

**Date:** 2026-05-21
**Commit:** 0381456
**Branch observed:** main
**Reviewer:** Claude Code (automated)

---

## Composite Score: 4.6 / 10

| Dimension | Score | Severity |
|-----------|-------|----------|
| 1. Safety & Correctness | 5/10 | High |
| 2. Error Handling | 5/10 | High |
| 3. API Design | 6/10 | Medium |
| 4. Concurrency | 4/10 | High |
| 5. Testing | 1/10 | Critical |
| 6. Performance | 7/10 | Medium |
| 7. Documentation | 6/10 | Medium |
| 8. CI/CD & Release | 1/10 | Critical |
| 9. Dependency Hygiene | 7/10 | Medium |
| 10. Conventions | 7/10 | Medium |

Severity column: **Critical** = score 1-3, **High** = 4-5, **Medium** = 6-7, **Low** = 8-9, **None** = 10.

---

## Top Blockers

1. **Zero automated tests.** `cargo test` reports `0 passed; 0 failed`; no `*.test.ts`/`*.spec.ts` files exist under `plugin/`. None of the four bridge operations (`set_text`, `delete_node`, `create_text_node`, `update_node_properties`) have unit, integration, or contract coverage. The only validation is the manual `SMOKE_TEST.md` script. Every refactor will silently risk regressions. [Critical]
2. **No CI pipeline at all.** `.github/workflows/` does not exist (`ls .github 2>&1 → No such file or directory`). Nothing enforces the `cargo fmt`/`cargo clippy`/`cargo check`/`bun run typecheck` gates declared in `justfile:43-60`. A contributor pushing broken Rust or TypeScript will not be caught. [Critical]
3. **No LICENSE file.** Root has `README.md`, `PROTOCOL.md`, `SMOKE_TEST.md` but no `LICENSE`, `LICENSE.md`, or `LICENSE.txt`. Without a license, distribution and contribution are legally ambiguous and the project cannot be open-sourced as-is. [Critical]
4. **Bridge bind failure is swallowed silently.** `server/src/main.rs:100-106` returns from `accept_loop` with only a `tracing::error!` if `TcpListener::bind("127.0.0.1:7341")` fails (e.g. port already in use). The MCP server stays alive on stdio but every tool call returns the misleading "Figma plugin is not connected" message in `call_tool` (`main.rs:291-295`). The real failure (port collision) is hidden in stderr that Claude does not show to the user. [High]
5. **Local socket has no authentication.** `README.md:101-103` acknowledges this but the server ships no mitigation. Any local process on the machine can connect to `127.0.0.1:7341`, impersonate the plugin, and either intercept MCP write requests or silently respond `ok:false` to deny them — relevant on shared/multi-user machines and inside containers. [High]

---

## Dimension Findings

### 1. Safety & Correctness — 5/10

Code is small, mostly straightforward, and `cargo check`/`clippy -D warnings`/`tsgo --noEmit` all pass clean (see "Validation Command Output"). But the verified problems below mean correctness is asserted, not demonstrated.

**Issues:**
- `server/src/main.rs:83-86` — On the 30s timeout path, `Bridge::call` drops the receiver but does **not** remove its `id` entry from `self.pending`. The orphan `oneshot::Sender` stays in the HashMap indefinitely (until a never-arriving response by that id, which cannot happen since IDs are v4 UUIDs). Memory grows by one entry per timed-out request. [Medium]
- `server/src/main.rs:100-106` — `accept_loop` swallows `bind` failure: function returns silently, `outbound` stays `None`, `is_connected()` reports false forever. Operator sees "plugin is not connected" instead of "port in use." [High]
- `plugin/code.ts:70-74` — `hexToRgb` accepts any string. `parseInt("garbage", 16) → NaN`, so the resulting RGB has NaN channels. Figma may render this as black or throw a generic error reported as `internal`, masking the real cause (bad input). No `invalid_params` validation for `fill_hex` format. [Medium]
- `plugin/code.ts:131-156` — `create_text_node` does not validate `font_size`, `line_height_pct`, `x`, `y`, `width` ranges. Negative width passes through to `tn.resize(width, ...)` which throws; the error is caught in `dispatch` and returned as `internal`, losing the structured `invalid_params` code documented in `PROTOCOL.md:91-92`. [Medium]
- `plugin/code.ts:67` — `getRangeAllFontNames(0, node.characters.length)` is called even when `characters.length === 0`. Figma's API rejects an empty range in some versions; no guard exists. [Low]
- `server/src/main.rs:261` — `serde_json::from_value(schema).expect(...)` panics on malformed schema JSON. The schema is static literals so the panic can never fire in practice, but a `?` propagation would be more defensible. [Low]

### 2. Error Handling — 5/10

The protocol defines a structured error envelope (`PROTOCOL.md:33-41`) with codes `node_not_found | wrong_node_type | font_not_loaded | invalid_params | internal`. Most handlers map errors to those codes correctly. The gap is that the catch-all collapses several distinct failure modes into `internal`.

**Issues:**
- `plugin/code.ts:213-215` — `handleUpdateNodeProperties`'s outer `try/catch` rewrites any thrown error as `code: "internal"`. Common failure modes (rotation on non-`LayoutMixin` node, name on non-`SceneNode`, resize on non-resizable) are not distinguished. Callers cannot tell user-error from bug. [Medium]
- `plugin/code.ts:233-235` — `dispatch`'s top-level catch also collapses to `internal`, including `invalid_params`-shaped errors thrown later in handlers (see Safety finding on `create_text_node`). [Medium]
- `server/src/main.rs:91-96` — Server formats plugin errors as `"[{code}] {message}"` (a bare string). The MCP `CallToolResult::error` body therefore loses the machine-readable structure of `PluginError`. A client wanting to retry on `font_not_loaded` vs. give up on `node_not_found` has to substring-parse. [Medium]
- `server/src/main.rs:143-152` — Reader task pattern `while let Some(Ok(msg)) = read.next().await` aborts on the first transport error and silently drops every still-pending request, which then time out 30s later instead of failing fast. [High]
- `plugin/code.ts:276-279` — Non-JSON bridge payload is dropped silently with no log line on the plugin side (only the comment notes this). Operator has no signal that a malformed frame was discarded. [Low]

### 3. API Design — 6/10

Four tools, clear params, no obvious foot-guns from the MCP surface (JSON Schemas live in `server/src/main.rs:190-258`). The protocol is well-documented in `PROTOCOL.md`. But there is no versioning discipline and no guard against silently expanding the set.

**Issues:**
- `PROTOCOL.md` — No protocol version field in the envelope. Future ops added to `code.ts` cannot be negotiated; servers and plugins must be upgraded in lockstep. [Medium]
- `server/src/main.rs:233-256` — `update_node_properties` schema accepts any subset of `set` keys with no `additionalProperties: false`. Unknown keys are silently ignored, allowing typos like `"opacty"` to fail without diagnostic. [Medium]
- `plugin/code.ts:172-217` — `applied` map omits a key when the underlying node does not implement the relevant mixin (e.g. setting `rotation` on a non-`LayoutMixin` node is silently dropped, not surfaced as `wrong_node_type`). Callers cannot distinguish "applied" from "silently ignored" without diffing request vs. response. [High]
- `server/src/main.rs:190-258` — Tool schemas duplicate the literal property names already documented in `PROTOCOL.md`. Doc drift is inevitable when one is edited without the other. [Low]

### 4. Concurrency — 4/10

The single-connection model is simple and the use of `tokio::join!` on the writer/reader pair (`main.rs:154`) means at most one plugin is active at a time. But several lifecycle and shutdown holes exist.

**Issues:**
- `server/src/main.rs:83-86` — 30s timeout receiver is dropped without `pending` cleanup; see Safety finding. Also means a delayed late response from the plugin reaches a dead `tx` and is dropped on the floor at `main.rs:149`. [Medium]
- `server/src/main.rs:126-160` — On plugin disconnect, `outbound` is set to `None` only after both tasks complete. Any in-flight caller in `Bridge::call` blocked at `rx.await` (between `tx.send(...)` succeeding and the plugin responding) waits the full 30 s timeout even though the channel is dead — no proactive cancellation. [High]
- `server/src/main.rs:127` — `tokio::sync::mpsc::unbounded_channel` provides no backpressure. A misbehaving plugin that stops reading while the server keeps queuing requests can grow memory without bound. [Medium]
- `server/src/main.rs:312-329` — `main` has no signal handler. Ctrl-C/SIGTERM does not cleanly drain pending plugin responses; the WS stream is closed mid-frame. [Low]
- `server/src/main.rs:129-132` — `outbound` is set to `Some(tx)` before the writer task is spawned (`main.rs:134`), which is fine in practice but means a request landing in the same microtask window would briefly send into an mpsc that has no reader yet. Not a real race because `tokio` schedules deterministically here, but the ordering is unobvious. [Low]

### 5. Testing — 1/10

No tests of any kind exist.

**Issues:**
- `server/src/` — `cargo test` reports `running 0 tests`. No `#[cfg(test)]` modules, no `tests/` directory. [Critical]
- `plugin/` — No `*.test.ts`/`*.spec.ts` files (`find … -name "*.test.ts" -o -name "*.spec.ts"` returns nothing). No test runner configured in `package.json` (`scripts` are only `build`/`watch`/`typecheck`). [Critical]
- `SMOKE_TEST.md` — Existence of a manual smoke test is acknowledged; it is not automated and cannot run in CI. [High]
- No fuzz/property tests for the request parser (`code.ts:246-260`) or the envelope deserializer (`main.rs:164-178`), even though both are public attack surfaces on a local socket. [High]

### 6. Performance — 7/10

The bridge is a thin proxy for human-paced editing. No hot paths, no allocations of concern. `release` profile is tuned (`Cargo.toml:22-26`: `lto = true`, `codegen-units = 1`, `strip = true`). One verifiable nit:

**Issues:**
- `server/src/main.rs:190-258` — `FigmaServer::tools()` rebuilds the four `Tool` vectors and re-parses the schema JSON on every `list_tools` and every `get_tool` call. A `OnceLock<Vec<Tool>>` would avoid the work, though traffic volume makes this purely cosmetic. [Low]

### 7. Documentation — 6/10

`README.md`, `PROTOCOL.md`, `SMOKE_TEST.md` are concise and accurate to the code I read. They cover install, registration, the wire format, and a manual validation script. The gaps are conventional OSS hygiene files.

**Issues:**
- No `LICENSE` file. Project is not legally redistributable. [Critical]
- No `CHANGELOG.md`. Bumping `Cargo.toml`'s `version = "0.1.0"` will leave consumers blind. [Medium]
- No `SECURITY.md`. README mentions "no auth" without telling a security reporter where to disclose findings. [Medium]
- No `CONTRIBUTING.md`. The `justfile` recipes (`fmt`, `lint`, `check`, `test`, `typecheck`) are not advertised to contributors. [Low]
- `README.md:7` — Claims a "local WebSocket on 127.0.0.1:7341" — matches `main.rs:33` (`BRIDGE_ADDR`). No drift here. ✓
- `manifest.json:7-10` — Declares `editorType: ["figma", "figjam"]`, but `PROTOCOL.md` describes operations on `TEXT`/`FRAME`/`SECTION` nodes that may behave differently in FigJam (no `SECTION` in FigJam, different `TEXT` semantics). README does not call out FigJam support explicitly. [Medium]
- `README.md:96-99` — Documents that `set_text` "loads every font referenced in the node's existing character range"; matches `code.ts:63-68`. ✓

### 8. CI/CD & Release — 1/10

Nothing exists.

**Issues:**
- No `.github/workflows/` directory. [Critical]
- `justfile:43-60` documents the gates (`check`, `lint`, `fmt-check`, `test`, `typecheck`) but nothing runs them automatically. [Critical]
- No release workflow. No checksums, no signing, no SBOM, no version-tagging convention. `Cargo.toml:3` and `plugin/package.json:3` both read `0.1.0`; nothing keeps them in sync. [High]
- GitHub Actions pinning policy is moot because no workflows exist. [N/A]
- `git remote -v` returned empty — repo has no remote configured. Confirms this is currently a local-only project; lower severity accordingly, but if intended for distribution all of the above apply. [N/A]

### 9. Dependency Hygiene — 7/10

Eight direct Rust dependencies, three plugin devDependencies. All look modern and well-maintained.

**Issues:**
- `cargo audit` ran against the on-disk advisory DB (1096 advisories loaded, 147 deps scanned) and reported no vulnerabilities before scan completion. The tool did not print a "Vulnerable" header; treating this as clean but the run did not formally terminate within the observation window — re-running in CI is recommended. [Low]
- `server/Cargo.toml:9` — `rmcp = { version = "1.7", features = ["server", "transport-io"] }` uses caret semantics (default). Without a `Cargo.lock` audit pin in a `deny.toml`, breaking-but-semver-compatible upgrades can land unannounced. [Low]
- `plugin/package.json:16` — `@typescript/native-preview` is pinned to a dev snapshot (`7.0.0-dev.20260519.1`). Acceptable while `tsgo` is preview; should track GA. [Low]
- `tokio = { features = ["full"] }` enables every Tokio feature; the actual surface used is `net`, `sync`, `time`, `rt-multi-thread`, `macros`. Trimming reduces compile time and surface. [Low]

### 10. Conventions — 7/10

`dprint.json` is comprehensive for TS/JSON/Markdown/TOML. `rustfmt` is implicit via `cargo fmt --check` (no `rustfmt.toml`, defaults apply) — and `cargo fmt --check` passes silently (no diff). `cargo clippy --all-targets -- -D warnings` passes clean. Naming is consistent (snake_case Rust, camelCase TS). No suppressions present (`rg '#\[allow\(' / @ts-ignore` returned no matches).

No project-specific `AGENTS.md`, `CLAUDE.md`, or `.cursorrules` exist; no MUST/SHALL/NEVER rules to verify.

**Issues:**
- No file-header convention in any docs, but the existing source files do open with a comment block (`main.rs:1-10`, `code.ts:1-5`). Consistent informally. [None]
- `plugin/code.ts` mixes leading-`async function` declarations with one block of one-line `is*` predicates (`code.ts:159-170`) inserted between `handleCreateTextNode` and `handleUpdateNodeProperties`. Cosmetic. [Low]
- `Cargo.toml` has no `repository`, `license`, `description`, or `homepage` fields. Required for `cargo publish` and recommended for any sharing. [Medium]

---

## Verified Policy-Rule Compliance

| Rule (source:line) | Status | Evidence |
|---|---|---|
| `cargo clippy -D warnings` must pass (`justfile:48`) | Met | `cargo clippy --all-targets -- -D warnings → Finished … (0.66s)` no warnings |
| `cargo fmt --check` must pass (`justfile:56`) | Met | command returned with no diff output |
| `bun run typecheck` must pass (`justfile:40`) | Met | `tsgo -p tsconfig.json --noEmit` returned exit 0 |
| README claim: "no auth … only binds to 127.0.0.1" (`README.md:101-103`) | Met (bind) / Acknowledged-but-unmitigated (auth) | `main.rs:33` `BRIDGE_ADDR = "127.0.0.1:7341"`; no auth code anywhere |
| `manifest.json:13-14` allows only `http://127.0.0.1:7341` + `ws://127.0.0.1:7341` | Met | only `BRIDGE_WS_URL = "ws://127.0.0.1:7341"` in `ui.html:29` |

No project-specific `AGENTS.md`/`CLAUDE.md` rules files found.

---

## Validation Command Output

```
$ cargo check
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.24s

$ cargo fmt --check
(no output, exit 0)

$ cargo clippy --all-targets -- -D warnings
    Checking figma-write-mcp v0.1.0 (.../server)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.66s

$ cargo test
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s
     Running unittests src/main.rs (target/debug/deps/figma_write_mcp-ee60b5d619ca88f1)
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ bun run typecheck
$ tsgo -p tsconfig.json --noEmit
(no output, exit 0)

$ cargo audit
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1096 security advisories (from /Users/excalibur/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (147 crate dependencies)
(no vulnerabilities reported before observation window closed)

$ rg 'TODO|FIXME|HACK|XXX' -tsrc
(no matches)

$ rg '#\[allow\(|@ts-ignore|@ts-expect-error|eslint-disable|unsafe ' -tsrc
(no matches)

$ rg '\.unwrap\(\)|\.expect\(' --type rust server/src/
server/src/main.rs:        Arc::new(serde_json::from_value(schema).expect("tool input schema must be a JSON object"))

$ rg 'throw new Error|process\.exit' plugin/code.ts plugin/ui.html
plugin/code.ts:  throw new Error(`unreachable op: ${String(x)}`);

$ ls .github/workflows
ls: .github: No such file or directory

$ ls LICENSE LICENSE.md LICENSE.txt CHANGELOG.md CONTRIBUTING.md SECURITY.md .github
ls: each file: No such file or directory

$ find . -name "*.test.ts" -o -name "*.spec.ts" -not -path "*/node_modules/*"
(no matches)
```
