# figma-write-mcp

A pair of programs that together give Claude (or any MCP client) the ability
to write to a Figma file:

- `server/` - a Rust MCP server that speaks MCP over stdio and forwards
  requests to the plugin over a local WebSocket on 127.0.0.1:7341.
- `plugin/` - a Figma plugin that runs inside the Figma desktop app, holds
  the live WebSocket, and executes operations against the Figma Plugin API.

The official Figma Dev Mode MCP server is read-only. This one is the write
counterpart. Run both at the same time and you get read + write.

## What it can do today

- set_text
- delete_node
- create_text_node
- update_node_properties (x, y, width, height, rotation, opacity, visible, name)

See PROTOCOL.md for the exact wire format.

## Prerequisites

- Rust toolchain (cargo) - install from https://rustup.rs
- Node.js 18+ (only to compile the plugin TypeScript to JavaScript)
- Figma desktop app

## Build

### Server

```
cd server
cargo build --release
```

The binary lands at `server/target/release/figma-write-mcp`.

### Plugin

```
cd plugin
bun install
bun run build
```

This compiles `code.ts` into `dist/code.js`. The plugin folder is
ready to import into Figma.

## Install the plugin in Figma

1. Open the Figma desktop app.
2. Menu: Plugins -> Development -> Import plugin from manifest...
3. Select `plugin/manifest.json`.
4. Open any Figma design file.
5. Right-click on the canvas -> Plugins -> Development -> Figma Write MCP
   Bridge. Leave the plugin window open while you want writes to work.
   You should see "connected to bridge" once the MCP server is running.

## Register the MCP server with Claude

Edit your Claude desktop config:

```
"$HOME/Library/Application Support/Claude/claude_desktop_config.json"
```

Under `mcpServers`, add:

```
"figma-write": {
  "command": "/absolute/path/to/figma-write-mcp/server/target/release/figma-write-mcp"
}
```

Fully quit Claude (Cmd+Q) and reopen it. The new tools should appear:
set_text, delete_node, create_text_node, update_node_properties.

## Order of operations at runtime

1. Start the Figma desktop app, open your design file.
2. Run the plugin (it tries to connect to ws://127.0.0.1:7341).
3. Claude launches the MCP server on demand; the server starts the WS
   listener and the plugin connects.
4. Claude calls a tool. The server sends `{id, op, params}` over WS to the
   plugin. The plugin executes against the Figma API and replies `{id, ok,
   result}` or `{id, ok: false, error}`.

If the plugin is not connected when Claude calls a tool, the server returns
a clear error rather than hanging.

## Limits and known sharp edges

- The plugin must remain open. Closing the plugin window severs the
  WebSocket and writes will fail until you reopen it.
- `set_text` loads every font referenced in the node's existing character
  range before mutating. Missing fonts surface as `font_not_loaded` errors;
  install the font locally or switch the node to a font you have.
- Coordinates are local to the node's parent, matching the Figma API.
- The protocol has no auth. It only binds to 127.0.0.1, but anything on
  your machine could connect. Don't expose port 7341 externally.
- Only one plugin instance should be open at a time. Multiple connections
  are not multiplexed.

## File layout

```
figma-write-mcp/
  PROTOCOL.md          wire protocol reference
  README.md            this file
  server/
    Cargo.toml
    src/main.rs
  plugin/
    manifest.json
    code.ts            sandbox code (compile to code.js)
    ui.html            iframe that owns the WebSocket
    package.json
    tsconfig.json
```
