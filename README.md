# figma-write-mcp

A pair of programs that together give Claude (or any MCP client) the ability
to write to a Figma file:

- `server/` — a Rust MCP server that speaks MCP over stdio and forwards
  requests to the plugin over an **authenticated** local WebSocket on
  `127.0.0.1:7341`.
- `plugin/` — a Figma plugin that runs inside the Figma desktop app, holds
  the live WebSocket, and executes operations against the Figma Plugin API.

The official Figma Dev Mode MCP server is read-only. This one is the write
counterpart. Run both at the same time and you get read + write.

## What it can do today

- `set_text`
- `delete_node`
- `create_text_node`
- `update_node_properties` (x, y, width, height, rotation, opacity, visible, name)

See [`PROTOCOL.md`](PROTOCOL.md) for the exact wire format and
[`SECURITY.md`](SECURITY.md) for the threat model.

## Prerequisites

- Rust toolchain (`cargo`) — install from <https://rustup.rs>.
- [`bun`](https://bun.sh) ≥ 1.3.0 to compile / bundle the plugin TypeScript.
- Figma desktop app.

## Build

### Server

```sh
cd server
cargo build --release
```

The binary lands at `server/target/release/figma-write-mcp`.

### Plugin

```sh
cd plugin
bun install
bun run build
```

This bundles `code.ts` and its imports into `dist/code.js`. The plugin
folder is then ready to import into Figma.

## Install the plugin in Figma

1. Open the Figma desktop app.
2. Menu → Plugins → Development → Import plugin from manifest…
3. Select `plugin/manifest.json`.
4. Open any Figma design file.
5. Right-click on the canvas → Plugins → Development → **Figma Write MCP
   Bridge**. Leave the plugin window open while you want writes to work.

## Register the MCP server with Claude

Edit your Claude desktop config:

```sh
"$HOME/Library/Application Support/Claude/claude_desktop_config.json"
```

Under `mcpServers`, add:

```json
"figma-write": {
  "command": "/absolute/path/to/figma-write-mcp/server/target/release/figma-write-mcp"
}
```

Fully quit Claude (Cmd+Q) and reopen it. The new tools should appear:
`set_text`, `delete_node`, `create_text_node`, `update_node_properties`.

## First-launch handshake (one-time secret)

The bridge requires an authenticated handshake. On its first launch the
server generates a random 32-byte secret and stores it (mode `0600`) at:

- macOS: `~/Library/Application Support/figma-write-mcp/secret`
- Linux: `$XDG_CONFIG_HOME/figma-write-mcp/secret` (or `~/.config/figma-write-mcp/secret`)
- Override either with `FIGMA_WRITE_MCP_HOME=/some/dir`.

The same secret is also printed to the server's stderr on the first launch
(look for `generated new bridge secret …`). Copy it once and paste it into
the Figma plugin window's "bridge secret" field; the plugin remembers it
across restarts via `figma.clientStorage`.

If you ever lose the secret, delete the file above and the server will
generate a new one on its next launch; the plugin will prompt you to paste
the new value.

## Order of operations at runtime

1. Start the Figma desktop app, open your design file.
2. Run the plugin (it tries to connect to `ws://127.0.0.1:7341`).
3. Claude launches the MCP server on demand; the server starts the WS
   listener and the plugin connects.
4. The plugin's first frame is the `hello` envelope; the server verifies
   the secret in constant time and replies with `hello_ok`.
5. Claude calls a tool. The server sends `{id, op, params}` over WS to the
   plugin. The plugin executes against the Figma API and replies
   `{id, ok, result}` or `{id, ok: false, error: {code, message}}`.

If the plugin is not connected when Claude calls a tool, the server returns
a structured `{code: "plugin_not_connected"}` error rather than hanging.
If the plugin disconnects mid-request, every in-flight caller receives a
`{code: "plugin_disconnected"}` error immediately rather than waiting for
the 30 s timeout.

## Limits and known sharp edges

- The plugin must remain open. Closing the plugin window severs the
  WebSocket and writes will fail until you reopen it.
- `set_text` loads every font referenced in the node's existing character
  range before mutating. Missing fonts surface as `font_not_loaded` errors;
  install the font locally or switch the node to a font you have.
- Coordinates are local to the node's parent, matching the Figma API.
- Only one plugin instance should be open at a time. Multiple concurrent
  connections are not multiplexed.
- The bridge is loopback-only and authenticated, but a privileged local
  attacker who can read your home directory can read the secret. See
  [`SECURITY.md`](SECURITY.md) for the full threat model.

## FigJam vs Figma

`manifest.json` declares both `figma` and `figjam` as editor types, but the
tools differ in scope:

| Operation                | Figma | FigJam              |
| ------------------------ | ----- | ------------------- |
| `set_text`               | ✅    | ✅ (sticky / shape) |
| `delete_node`            | ✅    | ✅                  |
| `create_text_node`       | ✅    | ⚠️ (no `FRAME` parents — use `PAGE` or a `SECTION`) |
| `update_node_properties` | ✅    | ⚠️ (`rotation`, `width`/`height` are no-ops on FigJam stickies — surfaced as `ignored.<key>: "wrong_node_type"`) |

## File layout

```
figma-write-mcp/
  LICENSE              MIT
  README.md            this file
  PROTOCOL.md          wire protocol reference
  SECURITY.md          threat model + disclosure address
  CHANGELOG.md         Keep-a-Changelog log
  CONTRIBUTING.md      gates and conventions
  SMOKE_TEST.md        manual round-trip test
  justfile             task runner
  dprint.json          formatter config
  .github/workflows/   ci.yml, release.yml
  server/
    Cargo.toml
    src/main.rs
  plugin/
    manifest.json
    code.ts            sandbox code (bundled into dist/code.js)
    helpers.ts         pure helpers (testable)
    helpers.test.ts    unit tests (bun test)
    ui.html            iframe that owns the WebSocket
    package.json
    tsconfig.json      plugin typecheck
    tsconfig.test.json test typecheck
```
