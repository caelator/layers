# Layers CLI Brain Wiring Plan

## Architecture

Layers daemon spawns CLI coding agents as child processes, captures stdout, streams it back through the portal WebSocket.

## Config: layers.toml

```toml
[daemon]
bind_address = "127.0.0.1"
port = 18791

[agent]
workspace = "/Users/bri/.openclaw/workspace"
default_model = "opus"
timezone = "America/Bogota"

[brains.opus]
cli = "/Users/bri/.local/bin/claude"
args = ["--permission-mode", "bypassPermissions", "--print", "--model", "opus"]
output = "stream-json"  # ndjson with type field
session_arg = "--session-id"
session_key = "session_id"
needs_pty = false

[brains.sonnet]
cli = "/Users/bri/.local/bin/claude"
args = ["--permission-mode", "bypassPermissions", "--print", "--model", "sonnet"]
output = "stream-json"
session_arg = "--session-id"
session_key = "session_id"
needs_pty = false

[brains.codex]
cli = "/Users/bri/.local/bin/codex"
args = ["--full-auto"]
output = "text"
needs_pty = true

[brains.gemini]
cli = "/Users/bri/.local/bin/gemini"
args = ["--prompt"]
output = "text"
needs_pty = true
```

## Implementation Steps

### Step 1: BrainConfig in layers-core
- New struct: `BrainConfig { cli, args, output, session_arg, session_key, needs_pty }`
- Parse from `layers.toml` under `[brains.*]`
- Each brain has a short name (opus, sonnet, codex, gemini)

### Step 2: BrainDispatcher in layers-runtime (new file)
- `BrainDispatcher` holds the brain configs and session state
- `dispatch(brain_name, prompt, session_id, workdir) -> impl Stream<BrainEvent>`
- For `output = "stream-json"`: spawn process, parse ndjson lines, emit `BrainEvent::Token(content)` or `BrainEvent::Done(session_id)`
- For `output = "text"`: spawn PTY, capture output, emit `BrainEvent::Token(chunk)`
- Map `session_arg` to CLI arg for session continuity (e.g. `--session-id <id>`)
- BrainEvent enum: `Token(String)`, `Done { session_id: Option<String> }`, `Error(String)`

### Step 3: Wire /ws and /api/chat to BrainDispatcher
- DaemonRunner holds `Arc<BrainDispatcher>`
- Pass to Gateway via AppState
- WebSocket handler: receive text → dispatch to default brain → stream BrainEvents back as WebSocket messages
- POST /api/chat: same but via SSE (Server-Sent Events)
- Model field in request maps to brain name

### Step 4: Wire into existing agent_loop.rs
- Agent loop already has the tool execution cycle
- Replace the ModelProvider calls with BrainDispatcher dispatch
- Agent loop becomes: intake → dispatch to brain → parse response → check for tool calls → execute tools → loop

### Step 5: Portal model selector maps to brains
- GET /api/models returns available brains from config
- Frontend model selector shows: Opus, Sonnet, Codex, Gemini
- Selected model sent with each message

## Key Design Decisions

1. **Claude Code is the primary brain** — `--print` mode gives clean stream-json output, no PTY needed, fast
2. **Session continuity** via `--session-id` for Claude, context re-injection for others
3. **Streaming first** — BrainEvent::Token chunks flow through WebSocket in real-time
4. **Workdir** — brain processes run in the workspace directory, same as OpenClaw
5. **No direct API calls** — everything goes through CLIs, keeping Layers lightweight

## File Changes

| File | Change |
|------|--------|
| `crates/layers-core/src/config.rs` | Add `brains: HashMap<String, BrainConfig>` to LayersConfig |
| `crates/layers-core/src/types.rs` | Add BrainConfig, BrainEvent structs |
| `crates/layers-runtime/src/brain.rs` | NEW — BrainDispatcher implementation |
| `crates/layers-daemon/src/gateway.rs` | Wire /ws and /api/chat to BrainDispatcher |
| `crates/layers-daemon/src/lifecycle.rs` | Initialize BrainDispatcher from config |
| `layers.toml` | Add [brains.*] sections |

## Execution Order

1. BrainConfig types + config parsing (15 min)
2. BrainDispatcher with Claude --print support (30 min)  
3. Wire gateway WebSocket handler (20 min)
4. Wire /api/chat SSE handler (15 min)
5. Update /api/models to return brains (5 min)
6. Test end-to-end: portal → daemon → claude CLI → stream back (10 min)
7. Add codex/gemini PTY support (20 min)
