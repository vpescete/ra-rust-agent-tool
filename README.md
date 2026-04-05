# RA — Rust Agent Orchestrator for Claude Code

RA is a high-performance agent orchestration runtime written in Rust that integrates with Claude Code as an MCP server. It enables parallel multi-agent execution, workflow pipelines with DAG dependencies, shared context between agents, checkpointing, and real-time metrics tracking.

RA does not replace Claude Code — it extends it with the orchestration layer it lacks.

## What RA solves

Claude Code runs agents sequentially and in isolation. RA adds:

- **Parallel execution** — run N agents concurrently with rate limiting and backpressure
- **DAG workflows** — define multi-step pipelines where steps depend on each other
- **Shared context** — agents pass results to downstream agents via variable substitution
- **Checkpoint/resume** — save workflow state to SQLite, resume after failures
- **Async execution** — launch agents in background, poll for progress
- **Workflow templates** — pre-built workflows for common tasks (code review, bug hunt, etc.)
- **Metrics tracking** — token usage, cost, duration per agent

## Requirements

- **Rust** 1.80+ (tested with 1.93)
- **Claude Code** CLI installed (`claude` command available)
- **Claude subscription** (Pro/Max) or Anthropic API key
- macOS or Linux

## Installation

### Build from source

```bash
git clone <repo-url> rust-agent-tool
cd rust-agent-tool
cargo build --release
```

This produces two binaries:
- `target/release/ra` — standalone CLI
- `target/release/ra-mcp-server` — MCP server for Claude Code

### Register as MCP server in Claude Code

To make RA available in **all projects**:

```bash
claude mcp add ra /path/to/rust-agent-tool/target/release/ra-mcp-server -s user
```

To make RA available in the **current project only**:

```bash
claude mcp add ra /path/to/rust-agent-tool/target/release/ra-mcp-server
```

Verify it's connected:

```bash
claude mcp list
# Should show: ra: /path/to/ra-mcp-server - Connected
```

### Optional: install CLI globally

```bash
cargo install --path crates/ra-cli
```

## Usage as MCP server (recommended)

Once registered, RA tools are available inside any Claude Code session. Claude Code will automatically use them when appropriate, or you can request them explicitly.

### 11 MCP tools

| Tool | Description |
|------|-------------|
| `ra_run_agents` | Run multiple agents in parallel (blocking, waits for all) |
| `ra_run_agents_async` | Launch agents in background, return `run_id` immediately |
| `ra_get_run_status` | Poll async run progress (completed/running/failed per agent) |
| `ra_get_agent_output` | Get full output of a specific agent from an async run |
| `ra_run_workflow` | Execute a YAML workflow with DAG dependencies |
| `ra_run_template` | Run a pre-built workflow template by name |
| `ra_list_templates` | List available workflow templates |
| `ra_agent_status` | Show currently active runs |
| `ra_metrics` | Metrics from last execution |
| `ra_history` | List past executions |
| `ra_checkpoint_list` | List checkpoints for a workflow |

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

**Custom workflow:**
> "Run this workflow: first analyze the database schema, then generate migration scripts, then validate them"

Claude Code calls `ra_run_workflow` with inline YAML.

## Workflow templates

RA ships with 5 pre-built templates. List them with `ra_list_templates`.

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

# TUI dashboard (basic)
ra dashboard
```

## Configuration

RA uses `~/.ra/config.toml` for runtime configuration. If it doesn't exist, defaults are used.

```toml
[runtime]
max_concurrency = 4                    # max parallel agents
claude_binary = "/opt/homebrew/bin/claude"
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
┌──────────────────────────────────────────���
│         Claude Code (chat + LLM)         │
└─────────────────┬────────────────────────┘
                  │ MCP (JSON-RPC over stdio)
┌─────────────────▼────────────────────────┐
│          ra-mcp-server (Rust)            │
│  11 tools, 5 templates, async runs       │
├──────────────────────────────────────────┤
│              ra-engine                   │
│  Agent Manager │ DAG Engine │ Scheduler  │
│  Context Mgr   │ Process    │ Stream     │
├──────────────────────────────────────────┤
│    ra-store (SQLite)  │  ra-observe      │
│    Checkpoints        │  Metrics, TUI    │
├──────────────────────────────────────────┤
│              ra-core                     │
│  Types, traits, errors, validation       │
└──────────────────────────────────────────┘
         │         │         │
    claude -p  claude -p  claude -p
    (subprocess per agent)
```

### Crate structure

| Crate | Purpose |
|-------|---------|
| `ra-core` | Domain types, traits, errors, DAG validation |
| `ra-engine` | Subprocess management, DAG execution, scheduling, shared context |
| `ra-store` | SQLite persistence for checkpoints and execution history |
| `ra-observe` | Metrics collection, TUI dashboard, JSON/CSV export |
| `ra-cli` | Standalone CLI binary |
| `ra-mcp` | MCP server binary for Claude Code integration |

## Development

```bash
# Build
cargo build

# Test (65 tests)
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo fmt --all

# Build release
cargo build --release
```

### Adding a new template

1. Create a YAML file in `templates/`
2. Add an `include_str!` entry in `crates/ra-mcp/src/handler.rs` → `embedded_templates()`
3. Rebuild: `cargo build --release -p ra-mcp`

User templates can also be placed in `~/.ra/templates/` without recompiling.

## How it works under the hood

1. Claude Code sends a JSON-RPC request to `ra-mcp-server` via stdin
2. The handler parses tool arguments and decides what to do
3. For `ra_run_agents`: spawns N `claude -p --output-format stream-json` subprocesses
4. A tokio semaphore limits concurrency; a token bucket handles rate limiting
5. Each subprocess streams JSONL events which are parsed for metrics
6. Results are collected, formatted as markdown, and returned via JSON-RPC
7. For async runs: background tokio tasks update shared state; polling reads it
8. For workflows: DAG engine resolves dependencies, substitutes variables, handles failures
9. Checkpoints are saved to SQLite for crash recovery

## License

MIT
