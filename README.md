# RA — Rust Agent Orchestrator for Claude Code

RA is a high-performance agent orchestration runtime written in Rust that integrates with Claude Code as an MCP server. It enables parallel multi-agent execution, workflow pipelines with DAG dependencies, shared context between agents, checkpointing, async execution with polling, workflow templates, a live TUI dashboard, and custom workflow creation.

RA does not replace Claude Code — it extends it with the orchestration layer it lacks.

## What RA solves

Claude Code runs agents sequentially and in isolation. RA adds:

- **Parallel execution** — run N agents concurrently with rate limiting and backpressure
- **DAG workflows** — define multi-step pipelines where steps depend on each other
- **Shared context** — agents pass results to downstream agents via variable substitution
- **Checkpoint/resume** — save workflow state to SQLite, resume after failures
- **Async execution** — launch agents in background, poll for progress
- **Workflow templates** — 5 pre-built workflows for common tasks (code review, bug hunt, etc.)
- **Workflow builder** — validate and save custom workflows as reusable templates
- **Live TUI dashboard** — monitor agents in real-time via Unix socket IPC
- **Slash commands** — 10 `/ra-*` commands for quick access in Claude Code
- **Metrics tracking** — token usage, cost, duration per agent

## Requirements

- **Rust** 1.80+ (tested with 1.93)
- **Claude Code** CLI installed (`claude` command available in PATH)
- **Claude subscription** (Pro/Max) or Anthropic API key
- macOS or Linux

## Installation

### Quick install

```bash
curl -sSL https://raw.githubusercontent.com/vpescete/ra-rust-agent-tool/main/install.sh | bash
```

### Build from source

```bash
git clone https://github.com/vpescete/ra-rust-agent-tool.git
cd ra-rust-agent-tool
cargo build --release
```

This produces two binaries:
- `target/release/ra` — standalone CLI
- `target/release/ra-mcp-server` — MCP server for Claude Code

### Install CLI globally

```bash
cargo install --path crates/ra-cli
```

### Register as MCP server in Claude Code

To make RA available in **all projects**:

```bash
claude mcp add ra /path/to/ra-mcp-server -s user
```

To make RA available in the **current project only**:

```bash
claude mcp add ra /path/to/ra-mcp-server
```

Verify it's connected:

```bash
claude mcp list
# Should show: ra: /path/to/ra-mcp-server - Connected
```

### Install slash commands (optional)

Copy the slash command files to your Claude Code config:

```bash
cp -r commands/ra-*.md ~/.claude/commands/
```

This enables `/ra-review`, `/ra-bug-hunt`, `/ra-templates`, etc. in Claude Code.

## Usage as MCP server (recommended)

Once registered, RA tools are available inside any Claude Code session. Claude Code will automatically use them when appropriate, or you can request them explicitly.

### 13 MCP tools

| Tool | Description |
|------|-------------|
| `ra_run_agents` | Run multiple agents in parallel (blocking, waits for all) |
| `ra_run_agents_async` | Launch agents in background, return `run_id` immediately |
| `ra_get_run_status` | Poll async run progress (completed/running/failed per agent) |
| `ra_get_agent_output` | Get full output of a specific agent from an async run |
| `ra_run_workflow` | Execute a YAML workflow with DAG dependencies |
| `ra_run_template` | Run a pre-built workflow template by name |
| `ra_list_templates` | List available workflow templates |
| `ra_validate_workflow` | Validate a workflow YAML without executing it (checks DAG, deps, variables) |
| `ra_save_workflow` | Save a workflow YAML as a reusable template in `~/.ra/templates/` |
| `ra_agent_status` | Show currently active runs |
| `ra_metrics` | Metrics from last execution |
| `ra_history` | List past executions |
| `ra_checkpoint_list` | List checkpoints for a workflow |

### Slash commands

Type `/ra-` in Claude Code to see all available commands:

| Command | Description |
|---------|-------------|
| `/ra-review` | Pre-PR review (security, quality, test coverage) |
| `/ra-bug-hunt` | 5 agents hunt bugs from different angles |
| `/ra-onboard` | Generate codebase onboarding guide |
| `/ra-migrate` | Create migration plan to a new technology |
| `/ra-test-gen` | Generate and validate unit tests |
| `/ra-templates` | List available workflow templates |
| `/ra-create-workflow` | Interactive workflow builder |
| `/ra-agents` | Run parallel agents on any task |
| `/ra-status` | Check active runs and recent history |
| `/ra-validate` | Validate a workflow YAML |

### Examples in Claude Code

**Parallel review:**
> "Do a security, performance, and architecture review of this project in parallel, then synthesize the results"

Claude Code calls `ra_run_agents` with 3 agents automatically.

**Using a template:**
> "Run the pre-pr-review template on this project"

Claude Code calls `ra_run_template` with `template: "pre-pr-review"`.

**Async execution with polling:**
> "Launch 5 bug-hunting agents in the background and check on them periodically"

Claude Code calls `ra_run_agents_async`, then polls with `ra_get_run_status`.

**Create and save a custom workflow:**
> "Create a workflow that analyzes the DB schema, generates migrations, and validates them. Save it as a template called db-migration."

Claude Code generates the YAML, calls `ra_validate_workflow`, then `ra_save_workflow`.

## Workflow templates

RA ships with 5 pre-built templates. List them with `ra_list_templates` or `/ra-templates`.

### pre-pr-review

3 parallel reviews (security, code quality, test coverage) followed by a synthesis step that produces a unified, prioritized action list.

**Parameters:**
- `target_path` — path to review (default: `.`)

### codebase-onboard

3 parallel analyses (structure, patterns, dependencies) followed by generation of a comprehensive developer onboarding guide.

**Parameters:**
- `target_path` — codebase root (default: `.`)

### bug-hunt

5 parallel bug hunters (error paths, concurrency, logic, edge cases, API contracts) followed by deduplication and prioritization.

**Parameters:**
- `target_path` — path to search (default: `.`)
- `bug_description` — optional symptom description

### migration-planner

Sequential pipeline: analyze current state, research target technology, generate phased migration plan.

**Parameters:**
- `target_path` — codebase to migrate (default: `.`)
- `target_tech` — target technology (required)
- `constraints` — migration constraints

### test-generator

Sequential pipeline: analyze testability, generate unit tests, validate generated tests.

**Parameters:**
- `target_path` — code to test (default: `.`)
- `test_framework` — test framework (default: auto-detect)

### Custom templates

You can create and save your own templates:

1. In Claude Code: `/ra-create-workflow` guides you interactively
2. Or manually: create a YAML file and save it with `ra_save_workflow`
3. Or place YAML files directly in `~/.ra/templates/`

## Custom workflows (YAML)

Define workflows as YAML files with DAG dependencies:

```yaml
name: "my-workflow"
description: "Custom analysis pipeline"

config:
  max_concurrency: 3
  max_total_budget_usd: 2.0

steps:
  - id: analyze
    name: "Analyze"
    prompt: "Analyze the code structure..."
    agent_config:
      model: sonnet
      allowed_tools: [Read, Glob, Grep]
    output_var: analysis

  - id: report
    name: "Report"
    prompt: |
      Based on this analysis:
      {{analysis}}
      Generate a report...
    depends_on:
      - step_id: analyze
        condition: Success
    output_var: report
```

### Step configuration

| Field | Description |
|-------|-------------|
| `id` | Unique step identifier |
| `name` | Display name |
| `prompt` | Prompt sent to the agent. Supports `{{variable}}` substitution |
| `agent_config.model` | Model: `sonnet`, `opus`, `haiku` |
| `agent_config.allowed_tools` | Claude Code tools the agent can use |
| `agent_config.max_budget_usd` | Cost cap per agent |
| `agent_config.priority` | `Low`, `Normal`, `High`, `Critical` |
| `depends_on` | List of step dependencies with condition (`Success`, `Failure`, `Always`) |
| `on_failure` | Failure policy: `skip`, `abort`, `retry`, or `fallback` |
| `output_var` | Variable name to store output for downstream steps |
| `inject_context` | If `true`, inject shared context into agent prompt |

### Failure handling

```yaml
on_failure:
  retry:
    max_attempts: 3

# or
on_failure: skip      # mark as skipped, continue
on_failure: abort     # stop entire workflow
on_failure:
  fallback:
    step_id: alternative_step
```

## Live TUI dashboard

Monitor agents in real-time while they run. The dashboard connects to the MCP server via Unix socket IPC.

**Terminal 1** — start Claude Code:
```bash
claude
> "Run the bug-hunt template"
```

**Terminal 2** — launch dashboard:
```bash
ra dashboard
```

The dashboard shows:
- **Overview tab** — agent metrics table + recent event log
- **Events tab** — full-screen scrollable event log with color coding
- Keyboard: `Tab` switch view, `j/k` scroll, `g/G` top/end, `q` quit
- Color coding: green=completed, red=error, yellow=rate limited, cyan=agent output

The dashboard requires Claude Code to be running (it creates the IPC socket at `~/.ra/ra.sock`).

## Standalone CLI usage

RA also works as a standalone CLI tool without Claude Code.

```bash
# Run a single agent
ra agent "Analyze this code for security issues" --model sonnet

# Execute a workflow
ra run workflow.yaml

# Show execution history
ra history

# Check status
ra status

# Resume from checkpoint
ra resume <checkpoint-id>

# TUI dashboard
ra dashboard
```

## Configuration

RA uses `~/.ra/config.toml` for runtime configuration. If it doesn't exist, defaults are used.

The `claude` binary is auto-detected from PATH. Override with the `CLAUDE_BINARY` environment variable or in config:

```toml
[runtime]
max_concurrency = 4                    # max parallel agents
claude_binary = "claude"               # auto-detected from PATH
default_model = "sonnet"

[rate_limit]
requests_per_minute = 50               # subscription tier limit
tokens_per_minute = 400000
burst_multiplier = 1.5
backoff_base_ms = 1000
backoff_max_ms = 60000

[persistence]
db_path = "~/.ra/ra.db"               # SQLite database
auto_checkpoint = true
checkpoint_interval_seconds = 60

[observability]
log_level = "info"
log_format = "pretty"                  # or "json"
metrics_export_path = "~/.ra/metrics/"

[defaults]
max_budget_usd = 5.0
token_budget = 200000
```

## Architecture

```
┌──────────────────────────────────────────┐
│         Claude Code (chat + LLM)         │
└─────────────────┬────────────────────────┘
                  │ MCP (JSON-RPC over stdio)
┌─────────────────▼────────────────────────┐
│          ra-mcp-server (Rust)            │
│  13 tools, 5 templates, async runs       │
│  IPC broadcaster (Unix socket)           │
├──────────────────────────────────────────┤
│              ra-engine                   │
│  Agent Manager │ DAG Engine │ Scheduler  │
│  Context Mgr   │ Process    │ Stream     │
├──────────────────────────────────────────┤
│    ra-store (SQLite)  │  ra-observe      │
│    Checkpoints        │  Metrics, TUI    │
│    History            │  IPC Client      │
├──────────────────────────────────────────┤
│              ra-core                     │
│  Types, traits, errors, DAG validation   │
│  IPC protocol, templates, run tracking   │
└──────────────────────────────────────────┘
         │         │         │
    claude -p  claude -p  claude -p
    (subprocess per agent)
```

### Crate structure

| Crate | Purpose |
|-------|---------|
| `ra-core` | Domain types, traits, errors, DAG validation, IPC protocol, template types |
| `ra-engine` | Subprocess management, DAG execution, scheduling, shared context |
| `ra-store` | SQLite persistence for checkpoints and execution history |
| `ra-observe` | Metrics collection, TUI dashboard with tabs, IPC client, JSON/CSV export |
| `ra-cli` | Standalone CLI binary |
| `ra-mcp` | MCP server binary, IPC broadcaster, template management |

## Development

```bash
# Build
cargo build

# Test (73 tests)
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all

# Build release
cargo build --release
```

### Adding a new embedded template

1. Create a YAML file in `templates/`
2. Add an `include_str!` entry in `crates/ra-mcp/src/handler.rs` -> `embedded_templates()`
3. Rebuild: `cargo build --release -p ra-mcp`

User templates can also be placed in `~/.ra/templates/` without recompiling, or saved via the `ra_save_workflow` MCP tool.

## How it works under the hood

1. Claude Code sends a JSON-RPC request to `ra-mcp-server` via stdin
2. The handler parses tool arguments and decides what to do
3. For `ra_run_agents`: spawns N `claude -p --output-format stream-json` subprocesses
4. A tokio semaphore limits concurrency; a token bucket handles rate limiting
5. Each subprocess streams JSONL events which are parsed for metrics
6. All events are forwarded via broadcast channel to the IPC broadcaster
7. The IPC broadcaster writes events to `~/.ra/ra.sock` for the TUI dashboard
8. Results are collected, formatted as markdown, and returned via JSON-RPC
9. For async runs: background tokio tasks update shared state; polling reads it
10. For workflows: DAG engine resolves dependencies, substitutes variables, handles failures
11. Shared context allows downstream steps to access upstream outputs
12. Checkpoints are saved to SQLite for crash recovery

## License

MIT
