# Wire protocol: MCP server <-> Figma plugin

The MCP server runs a local WebSocket listener on 127.0.0.1:7341. The Figma
plugin connects to it from inside Figma's UI iframe and stays connected for
the lifetime of the plugin window.

All messages are JSON. Every request from the server carries an id; the
plugin must echo that id on its response so requests can be matched.

## Request envelope (server -> plugin)

```
{
  "id": "uuid-v4",
  "op": "set_text" | "delete_node" | "create_text_node" | "update_node_properties",
  "params": { ... op-specific ... }
}
```

## Response envelope (plugin -> server)

```
{
  "id": "uuid-v4",
  "ok": true,
  "result": { ... op-specific ... }
}
```

or on failure:

```
{
  "id": "uuid-v4",
  "ok": false,
  "error": {
    "code": "node_not_found" | "wrong_node_type" | "font_not_loaded" | "invalid_params" | "internal",
    "message": "human readable explanation"
  }
}
```

## Operations

### set_text

Update the characters of an existing TEXT node.

params:
- node_id: string, required. Figma node id, e.g. "12:66".
- text: string, required. New text content.

result:
- node_id: string
- previous_text: string

Errors: node_not_found, wrong_node_type (node is not TEXT), font_not_loaded.

### delete_node

Delete any node by id.

params:
- node_id: string, required.

result:
- node_id: string
- parent_id: string, the parent the node was removed from.

Errors: node_not_found.

### create_text_node

Create a new TEXT node as a child of a given parent.

params:
- parent_id: string, required. Must be a node that can have children (FRAME, GROUP, COMPONENT, INSTANCE, PAGE, SECTION).
- text: string, required.
- x: number, optional. Local x within parent, defaults to 0.
- y: number, optional. Local y within parent, defaults to 0.
- width: number, optional. If provided, sets textAutoResize to "HEIGHT" and resizes to this width.
- font_family: string, optional. Defaults to "Inter".
- font_style: string, optional. Defaults to "Regular".
- font_size: number, optional. Defaults to 11.
- fill_hex: string, optional. e.g. "#111111". Defaults to "#111111".
- line_height_pct: number, optional. e.g. 140 for 140%.

result:
- node_id: string, id of the newly created text node.

Errors: node_not_found, wrong_node_type (parent cannot accept children),
font_not_loaded, invalid_params.

### update_node_properties

Update mutable properties on an existing node. All fields in params.set
are optional; only the ones provided are applied.

params:
- node_id: string, required.
- set: object, required. Any subset of:
  - x: number
  - y: number
  - width: number
  - height: number
  - rotation: number (degrees)
  - opacity: number (0..1)
  - visible: boolean
  - name: string

result:
- node_id: string
- applied: object, the subset of fields that were actually applied.

Errors: node_not_found, invalid_params.

## Heartbeat

Either side may send `{"op": "ping"}` (no id required). The other side
responds with `{"op": "pong"}`. The server uses this to detect a dropped
plugin connection and surface a clear error to Claude rather than hanging.
