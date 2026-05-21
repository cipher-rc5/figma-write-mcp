set shell := ["bash", "-cu"]

server_dir := justfile_directory() / "server"
plugin_dir := justfile_directory() / "plugin"
server_bin := server_dir / "target/release/figma-write-mcp"
server_bin_debug := server_dir / "target/debug/figma-write-mcp"

# List available recipes
default:
    @just --list

# Install plugin JS dependencies
install:
    cd {{plugin_dir}} && bun install

# Build server (release) and plugin
build: build-server build-plugin

# Build everything from a clean tree
rebuild: clean build

# Build the Rust MCP server in release mode
build-server:
    cd {{server_dir}} && cargo build --release

# Build the Rust MCP server in debug mode
build-server-debug:
    cd {{server_dir}} && cargo build

# Compile the Figma plugin TypeScript to dist/
build-plugin:
    cd {{plugin_dir}} && bun run build

# Watch and recompile the plugin on change
watch-plugin:
    cd {{plugin_dir}} && bun run watch

# Typecheck the plugin without emitting
typecheck:
    cd {{plugin_dir}} && bun run typecheck

# Run cargo check on the server
check:
    cd {{server_dir}} && cargo check

# Run cargo clippy on the server
lint:
    cd {{server_dir}} && cargo clippy --all-targets -- -D warnings

# Format Rust sources
fmt:
    cd {{server_dir}} && cargo fmt

# Verify Rust formatting (CI-friendly)
fmt-check:
    cd {{server_dir}} && cargo fmt -- --check

# Run cargo tests on the server
test:
    cd {{server_dir}} && cargo test

# Run the server directly (stdio MCP); for manual probing
run: build-server
    {{server_bin}}

# Launch mcp-inspector against the release server binary (stdio)
inspect: build-server
    bunx @modelcontextprotocol/inspector {{server_bin}}

# Launch mcp-inspector against the debug server binary (stdio, faster iteration)
inspect-debug: build-server-debug
    bunx @modelcontextprotocol/inspector {{server_bin_debug}}

# Print whether port 7341 (the plugin bridge) is bound
bridge-status:
    @lsof -nP -iTCP:7341 -sTCP:LISTEN || echo "port 7341 is free (server not running)"

claude_config := env_var('HOME') / "Library/Application Support/Claude/claude_desktop_config.json"

# Print the JSON snippet to paste into claude_desktop_config.json
claude-config-snippet: build-server
    @printf '%s\n' '"figma-write": {' '  "command": "{{server_bin}}"' '}'

# Open the Claude Desktop config file in $EDITOR (or TextEdit fallback)
claude-config-open:
    @[ -f "{{claude_config}}" ] || (mkdir -p "$(dirname "{{claude_config}}")" && echo '{ "mcpServers": {} }' > "{{claude_config}}")
    @${EDITOR:-open -t} "{{claude_config}}"

# Remove build artifacts
clean:
    cd {{server_dir}} && cargo clean
    rm -rf {{plugin_dir}}/dist
