# Companion Wrapper Specification

## Purpose

The `companion` binary is the user-facing entry point for axios-companion Tier 0. It is a shell wrapper that invokes the `claude` CLI with persona files, workspace directory, and (if present) mcp-gateway configuration pre-loaded — turning a stateless `claude` invocation into a persona-aware companion without any persistent process.

## ADDED Requirements

### Requirement: Binary Is A Pure Shell Wrapper

The `companion` binary MUST be implemented as a `pkgs.writeShellApplication`, not a compiled binary in a compiled language, for Tier 0. This keeps the implementation transparent, easily reviewable, and trivially debuggable.

#### Scenario: User inspects the binary

- **Given**: A user has `services.axios-companion.enable = true`
- **When**: The user runs `cat $(which companion)`
- **Then**: The output is a shell script they can read
- **And**: The script is less than 200 lines
- **And**: The script's logic is obvious from reading it

### Requirement: Persona Paths Are Resolved At Build Time

The wrapper MUST have persona file paths resolved at Nix evaluation time and embedded as literal `/nix/store/...` paths in the generated shell script. The wrapper MUST NOT scan directories, consult environment variables, or otherwise discover persona files at runtime. This keeps the script transparent (reading it reveals exactly which files will be loaded) and guarantees reproducibility.

The same build-time resolution applies to:

- The default `AGENT.md` path from `persona.basePackage`
- The default `USER.md` template path from `persona.basePackage`
- The user-provided `persona.userFile` path (when set)
- Each path in `persona.extraFiles`

The wrapper also bakes a `HAS_USER_FILE` shell variable (`0` or `1`) into the script at build time, recording whether `persona.userFile` was set. This variable is the single source of truth for first-run scaffolding decisions.

#### Scenario: Inspecting the generated script

- **Given**: A user has built the companion wrapper with default options
- **When**: The user runs `cat $(which companion)`
- **Then**: The script contains literal `/nix/store/...-persona-default/AGENT.md` and `/nix/store/...-persona-default/USER.md` paths
- **And**: The script contains a line like `HAS_USER_FILE=0`
- **And**: No environment-variable lookups or directory scans are used to locate persona files

#### Scenario: User file is wired at build time

- **Given**: `services.axios-companion.persona.userFile = ./my-context.md`
- **When**: The module builds the wrapper via the flake's `buildCompanion` helper
- **Then**: The generated script contains the literal store path of `./my-context.md`
- **And**: The generated script contains `HAS_USER_FILE=1`

### Requirement: Persona Resolution Order

The wrapper MUST assemble the system prompt from persona files in the following order, concatenated with blank-line separators:

1. Default base `AGENT.md` from `persona.basePackage`
2. User-provided `USER.md` from `persona.userFile` if set, OR the default `USER.md` template from `persona.basePackage` if not set
3. Each file in `persona.extraFiles` in the order they appear in the list

Later files in the order MAY add to, override, or contradict earlier files. The wrapper does not interpret file content — it concatenates.

#### Scenario: User has only default persona

- **Given**: `services.axios-companion.persona.userFile = null` and `persona.extraFiles = [ ]`
- **When**: The user runs `companion "hello"`
- **Then**: The system prompt passed to Claude is the concatenation of default `AGENT.md` and default `USER.md` template
- **And**: The total system prompt includes both files' contents in that order

#### Scenario: User overrides USER.md

- **Given**: `services.axios-companion.persona.userFile = ./my-context.md`
- **When**: The user runs `companion "hello"`
- **Then**: The system prompt contains default `AGENT.md` followed by the contents of `./my-context.md`
- **And**: The default `USER.md` template is NOT included

#### Scenario: User adds character persona files

- **Given**: `persona.userFile = ./my-context.md` and `persona.extraFiles = [ ./voice.md ./preferences.md ]`
- **When**: The user runs `companion "hello"`
- **Then**: The system prompt is: default `AGENT.md` + `./my-context.md` + `./voice.md` + `./preferences.md`

### Requirement: Workspace Directory Creation On First Run

The wrapper MUST ensure the workspace directory exists before invoking Claude. If the directory does not exist, the wrapper creates it along with a `README.md` explaining its purpose, and copies the default `USER.md` template into it if the user has not provided a user file via module options.

The decision of whether to copy the template MUST be made by reading the `HAS_USER_FILE` shell variable baked into the script at build time (see "Persona Paths Are Resolved At Build Time"). The wrapper MUST NOT inspect the filesystem or consult environment variables to make this decision.

#### Scenario: First-ever invocation on a fresh system

- **Given**: `services.axios-companion.workspaceDir = "$XDG_DATA_HOME/axios-companion/workspace"` (default)
- **And**: The directory does not exist
- **When**: The user runs `companion "hello"` for the first time
- **Then**: The directory is created
- **And**: A `README.md` is written explaining what the workspace is for
- **And**: A `USER.md` copy of the default template is written (only if `persona.userFile` is null)
- **And**: Claude is then invoked as normal

#### Scenario: First-run scaffolding uses the baked HAS_USER_FILE flag

- **Given**: The module was evaluated with `persona.userFile = null`
- **When**: The wrapper is built via `buildCompanion`
- **Then**: The generated script contains `HAS_USER_FILE=0`
- **And**: When the wrapper runs for the first time against an empty workspace, the default `USER.md` template is copied into the workspace

#### Scenario: User-provided file suppresses template copy

- **Given**: The module was evaluated with `persona.userFile = ./my-context.md`
- **When**: The wrapper is built via `buildCompanion`
- **Then**: The generated script contains `HAS_USER_FILE=1`
- **And**: When the wrapper runs for the first time against an empty workspace, no `USER.md` template is copied (the user's own file is referenced only as a system-prompt input, not scaffolded into the workspace)

#### Scenario: Workspace already exists

- **Given**: The workspace directory already exists with user files in it
- **When**: The user runs `companion "hello"`
- **Then**: The wrapper does NOT modify or overwrite any existing files in the workspace
- **And**: Claude is invoked normally

### Requirement: MCP Config Auto-Detection

The wrapper MUST check the following paths in order when `mcpConfigFile` is null (the default), and use the first one that exists:

1. `$XDG_CONFIG_HOME/mcp-gateway/claude_config.json`
2. `$XDG_CONFIG_HOME/mcp/mcp_servers.json`
3. `$HOME/.mcp.json`

If none exist, the wrapper MUST invoke Claude without `--mcp-config`. The wrapper MUST NOT error or warn if no MCP config is found — MCP tools are optional.

#### Scenario: User has mcp-gateway running

- **Given**: mcp-gateway has generated `$XDG_CONFIG_HOME/mcp-gateway/claude_config.json`
- **When**: The user runs `companion "list my github notifications"`
- **Then**: The wrapper detects the file and passes `--mcp-config <path>` to Claude
- **And**: Claude has access to every MCP server mcp-gateway exposes

#### Scenario: User explicitly sets mcpConfigFile

- **Given**: `services.axios-companion.mcpConfigFile = "/custom/path.json"`
- **When**: The user runs `companion "hello"`
- **Then**: The wrapper uses `/custom/path.json` and does NOT auto-detect
- **And**: If the file does not exist, the wrapper prints a warning to stderr but still invokes Claude

#### Scenario: No MCP config available

- **Given**: No MCP config files exist at any auto-detect path
- **And**: `mcpConfigFile` is null
- **When**: The user runs `companion "hello"`
- **Then**: The wrapper invokes Claude without `--mcp-config`
- **And**: The wrapper does not warn or error

### Requirement: Argument Passthrough

Arguments that the wrapper does not consume MUST be passed through to the underlying `claude` invocation verbatim. The wrapper MUST NOT intercept or rewrite Claude Code flags.

#### Scenario: User passes a one-shot prompt

- **Given**: The user runs `companion "what is the capital of Montana"`
- **When**: The wrapper invokes `claude`
- **Then**: The command is effectively `claude --append-system-prompt "<persona>" --add-dir "<workspace>" [--mcp-config <path>] -p "what is the capital of Montana"`

#### Scenario: User passes Claude Code flags

- **Given**: The user runs `companion --resume --model claude-haiku-4-5`
- **When**: The wrapper invokes `claude`
- **Then**: The `--resume` and `--model claude-haiku-4-5` flags are passed through to `claude`
- **And**: The wrapper's persona/workspace/mcp flags are ALSO applied

#### Scenario: User runs interactive mode

- **Given**: The user runs `companion` with no arguments
- **When**: The wrapper invokes `claude`
- **Then**: `claude` is started in interactive mode with persona/workspace/mcp pre-loaded

### Requirement: Exit Code Transparency

The wrapper MUST exit with the exit code of the underlying `claude` invocation. It MUST NOT swallow, translate, or wrap exit codes.

#### Scenario: Claude exits successfully

- **Given**: `claude -p "hello"` would exit 0
- **When**: `companion "hello"` runs
- **Then**: `companion` exits 0

#### Scenario: Claude errors

- **Given**: `claude -p "hello"` would exit with non-zero status
- **When**: `companion "hello"` runs
- **Then**: `companion` exits with the same non-zero status

### Requirement: No Persistent State Between Invocations

The wrapper MUST NOT write any state that persists between invocations beyond first-run workspace scaffolding. It MUST NOT maintain a lock file, cache, log, or session record of its own. (Claude Code's own `~/.claude/projects/` session storage is unaffected and is Claude Code's concern, not the wrapper's.)

#### Scenario: Two invocations in sequence

- **Given**: A user has the workspace already created
- **When**: The user runs `companion "first question"` and then `companion "second question"`
- **Then**: The wrapper creates no files, logs, or state between the two invocations
- **And**: Any conversation continuity is handled by Claude Code's own `--resume` flag
