# Security policy

## Threat model

`figma-write-mcp` runs as two processes on the same workstation:

- a Rust MCP server spawned by an MCP client (typically Claude Desktop) over stdio
- a Figma plugin running inside the Figma desktop app

The two processes communicate over a WebSocket bound to `127.0.0.1:7341`. The
server is not exposed externally and the manifest restricts the plugin's
network access to that single loopback origin.

### What is in scope

- Authentication and confidentiality of the bridge between the MCP server and
  the Figma plugin.
- Input validation of MCP tool parameters and of plugin response envelopes.
- Resource-exhaustion paths in either process (unbounded queues, memory leaks,
  request floods).

### What is out of scope

- A privileged local attacker who already has read access to the user's home
  directory. Such an attacker can read the bridge secret and impersonate the
  plugin, but they could equally read the Figma session cookie and act
  directly against Figma's API.
- The Figma desktop app's own sandbox. Bugs in Figma's plugin runtime are
  Figma's responsibility.
- The MCP client's process isolation. Bugs in Claude Desktop are Anthropic's.

## Authentication on the bridge

On first launch the server generates a random 32-byte secret and writes it to
`$HOME/Library/Application Support/figma-write-mcp/secret` (macOS) or
`$XDG_CONFIG_HOME/figma-write-mcp/secret` (Linux), with mode `0600`.

The Figma plugin's UI iframe reads the secret out of a one-time copy/paste
flow (the secret is also logged to the MCP server's stderr on first launch
with instructions). The plugin sends the secret as the first frame on a
fresh WebSocket connection:

```json
{"op": "hello", "protocol_version": 1, "secret": "<base64>"}
```

The server compares the secret in constant time. On mismatch the server
closes the connection and refuses to accept further frames; on match the
server sends `{"op": "hello_ok"}` and the connection is promoted to handling
tool requests.

The secret never leaves the local machine.

## Reporting a vulnerability

Please report security issues privately, **not** as a public GitHub issue.

- Email: `c0217636@gmail.com`
- Subject line: `[figma-write-mcp security]`
- Please include reproduction steps, the affected version (commit SHA), and
  the impact you observed. PGP key on request.

We aim to acknowledge reports within 72 hours.

## Disclosure policy

We follow coordinated disclosure. After a fix lands we will publish a
security advisory on GitHub describing the issue, the fix, and credit to the
reporter (unless the reporter prefers to remain anonymous).
