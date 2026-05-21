# Smoke test

Open a scratch Figma file with one frame in it so you have a known parent
to write into. The id of that frame is referred to below as `FRAME_ID`.

## 1. Plugin connectivity

Run the plugin. The status pill should switch from "disconnected" to
"connected to bridge" within ~2 seconds of starting the MCP server.

If it stays red:
- Confirm the server process is running and bound to 127.0.0.1:7341
  (`lsof -i :7341` on macOS).
- Click Reconnect in the plugin window.
- Check the plugin window's Log panel for the last error.

## 2. set_text (round-trip)

Pick any existing text node in the file. Note its id, e.g. "12:66".

Call:
```
set_text({ node_id: "12:66", text: "smoke test ok" })
```

Pass: the canvas updates immediately and the tool result contains the
node's previous_text. Fail: any non-zero `is_error` in the tool result, or
no visible change in the canvas.

## 3. create_text_node

```
create_text_node({
  parent_id: FRAME_ID,
  text: "hello from MCP",
  x: 16,
  y: 16,
  width: 300,
  font_family: "Inter",
  font_style: "Regular",
  font_size: 14,
  fill_hex: "#111111"
})
```

Pass: a new text node appears at (16, 16) inside FRAME_ID with the
expected copy. The tool result returns the new node's id - keep it for the
next two steps.

## 4. update_node_properties

Using the id from step 3:

```
update_node_properties({
  node_id: NEW_ID,
  set: { x: 100, y: 100, opacity: 0.5, name: "smoke target" }
})
```

Pass: the node moves to (100, 100), goes half-transparent, and its layer
name in the Figma sidebar changes to "smoke target".

## 5. delete_node

```
delete_node({ node_id: NEW_ID })
```

Pass: the node disappears from the canvas. A subsequent call with the same
id returns a `node_not_found` error.

## 6. Error envelopes

These should each return `is_error: true` with the matching error code:

- `set_text` on a non-text node id -> `wrong_node_type`
- any op on a nonexistent id -> `node_not_found`
- `set_text` with missing fields -> `invalid_params`

If all six pass, the bridge is healthy. Then point it at the proposal
template and run the actual edits.
