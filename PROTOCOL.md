# Wire protocol: MCP server <-> Figma plugin

The MCP server runs a local WebSocket listener on `127.0.0.1:7341`. The Figma
plugin connects to it from inside Figma's UI iframe and stays connected for
the lifetime of the plugin window.

All messages are JSON. The current protocol is **version 1** and is sent on
every envelope as `protocol_version: 1`. Servers and plugins MUST refuse
mismatched versions during the hello handshake.

## Hello handshake (first frame on every connection)

Plugin → server:

```json
{
  "op": "hello",
  "protocol_version": 1,
  "secret": "<base64-url-no-pad>"
}
```

The server reads its on-disk shared secret (see [`SECURITY.md`](SECURITY.md))
and compares it to the claimed secret in constant time. On success:

```json
{
  "op": "hello_ok",
  "protocol_version": 1
}
```

On failure:

```json
{
  "op": "hello_err",
  "code": "auth_failed | unsupported_version | invalid_params | timeout",
  "message": "human readable explanation"
}
```

After a `hello_err` the server closes the WebSocket. After `hello_ok` the
connection is promoted to handling tool requests.

## Request envelope (server -> plugin)

```json
{
  "id": "uuid-v4",
  "op": "set_text | delete_node | create_text_node | update_node_properties",
  "protocol_version": 1,
  "params": { ... op-specific ... }
}
```

## Response envelope (plugin -> server)

Success:

```json
{
  "id": "uuid-v4",
  "ok": true,
  "result": { ... op-specific ... }
}
```

Failure:

```json
{
  "id": "uuid-v4",
  "ok": false,
  "error": {
    "code": "node_not_found | wrong_node_type | font_not_loaded | invalid_params | internal",
    "message": "human readable explanation"
  }
}
```

The MCP server forwards the `error` object verbatim into the
`CallToolResult` so MCP clients can branch on `code` rather than parsing
text.

## Operations

### `set_text`

Update the characters of an existing TEXT node.

`params`:

- `node_id` — string, required. Figma node id, e.g. `"12:66"`.
- `text` — string, required. New text content.

`result`:

- `node_id` — string.
- `previous_text` — string.

Errors: `node_not_found`, `wrong_node_type` (node is not TEXT), `font_not_loaded`.

### `delete_node`

Delete any node by id.

`params`:

- `node_id` — string, required.

`result`:

- `node_id` — string.
- `parent_id` — string, the parent the node was removed from.

Errors: `node_not_found`.

### `create_text_node`

Create a new TEXT node as a child of a given parent.

`params`:

- `parent_id` — string, required. Must be a node that can have children
  (`FRAME`, `GROUP`, `COMPONENT`, `INSTANCE`, `PAGE`, `SECTION`).
- `text` — string, required.
- `x` — number, optional. Local x within parent, defaults to 0.
- `y` — number, optional. Local y within parent, defaults to 0.
- `width` — number, optional, > 0. If provided, sets `textAutoResize` to
  `"HEIGHT"` and resizes to this width.
- `font_family` — string, optional. Defaults to `"Inter"`.
- `font_style` — string, optional. Defaults to `"Regular"`.
- `font_size` — number, optional, > 0. Defaults to Figma's default.
- `fill_hex` — string, optional. Must match `^#?[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$`.
- `line_height_pct` — number, optional, > 0. e.g. `140` for 140 %.

`result`:

- `node_id` — string, id of the newly created text node.

Errors: `node_not_found`, `wrong_node_type` (parent cannot accept children),
`font_not_loaded`, `invalid_params`.

### `update_node_properties`

Update mutable properties on an existing node. All keys in `params.set` are
optional; only the ones provided are considered. Each provided key is
either applied (mirrored in `result.applied`) or skipped because the node
does not implement the relevant mixin (recorded in `result.ignored` with
the reason).

`params`:

- `node_id` — string, required.
- `set` — object, required (`additionalProperties: false`):
  - `x` — number
  - `y` — number
  - `width` — number > 0
  - `height` — number > 0
  - `rotation` — number (degrees)
  - `opacity` — number in `[0, 1]`
  - `visible` — boolean
  - `name` — string

`result`:

- `node_id` — string.
- `applied` — object, the subset of fields that were actually applied.
- `ignored` — object, keys that were dropped because the node does not
  implement the required mixin. Values are reason codes (currently always
  `"wrong_node_type"`).

Errors: `node_not_found`, `invalid_params`.

## Bridge-level errors

Errors that originate in the MCP server (rather than the plugin) are
reported with the same `{code, message}` shape so MCP clients can use one
parser:

| Code                    | Meaning                                                       |
| ----------------------- | ------------------------------------------------------------- |
| `plugin_not_connected`  | No plugin is connected to `127.0.0.1:7341`.                   |
| `plugin_disconnected`   | The plugin disconnected mid-request or before responding.    |
| `timeout`               | The plugin took more than 30 s to respond.                    |
| `send_failed`           | The outbound channel rejected the frame.                      |
| `auth_failed`           | The plugin's `hello.secret` did not match the on-disk secret. |
| `unsupported_version`   | The plugin claimed a `protocol_version` the server does not speak. |

## Heartbeat

Either side may send `{"op": "ping"}` after the hello handshake. The other
side responds with `{"ok": true, "result": {"pong": true}}`. The server
does not currently emit pings; the plugin echoes pings that arrive over
the bridge for completeness.
