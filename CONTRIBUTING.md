# Contributing

Thanks for considering a contribution. This project is small; the bar for
landing a change is "all gates green, one clear commit, and a reviewer can
re-derive intent from the diff."

## Toolchain

- Rust (stable, edition 2024) — install via [rustup](https://rustup.rs).
- [`bun`](https://bun.sh) ≥ 1.3.0 for the Figma plugin build and tests.
- [`just`](https://github.com/casey/just) for the task runner (optional but
  recommended; every recipe is also a one-line shell command).

## Recipes

```
just install        # bun install in plugin/
just build          # release server + bundled plugin
just check          # cargo check
just lint           # cargo clippy --all-targets -- -D warnings
just fmt            # cargo fmt
just fmt-check      # cargo fmt --check (CI-friendly)
just typecheck      # tsgo --noEmit on the plugin
just test           # cargo test
just test-plugin    # bun test in plugin/
just inspect-debug  # launch @modelcontextprotocol/inspector against the debug binary
just bridge-status  # check whether port 7341 is bound
```

## Before you push

Run the same gates CI runs. From the repo root:

```
just fmt-check
just lint
just test
just typecheck
just test-plugin
just build
```

Any failing gate will block CI.

## Commit hygiene

- One logical change per commit.
- Imperative subject ≤ 72 chars, no trailing period.
- Body wraps at 72 chars and explains *why* the change is needed, not what
  the diff already shows.
- No AI-tooling attribution lines.

## Reporting bugs

- Use the GitHub issue tracker for non-security bugs.
- For security issues, see [`SECURITY.md`](SECURITY.md).

## Adding a new MCP tool

1. Define the request/response shape in `PROTOCOL.md`.
2. Add the tool to `FigmaServer::tools_inner()` in `server/src/main.rs`,
   including a JSON Schema with `additionalProperties: false`.
3. Add the handler in `plugin/code.ts`, the matching branch in `dispatch`,
   and register the op name in `KNOWN_OPS`.
4. Add a smoke-test entry in `SMOKE_TEST.md`.
5. Add a unit test for any new pure helper in `plugin/helpers.test.ts` or
   `server/src/main.rs` `#[cfg(test)]`.
6. Bump the `## [Unreleased]` section of `CHANGELOG.md`.
