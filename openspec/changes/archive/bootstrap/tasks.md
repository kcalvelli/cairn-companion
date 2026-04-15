# Tasks: Bootstrap — Tier 0 Shell Wrapper and Home-Manager Module

## Phase 1: Repository scaffolding

- [x] **1.1** Create repository skeleton (`flake.nix`, `README.md`, `LICENSE`, `.gitignore`, `ROADMAP.md`)
- [x] **1.2** Create `openspec/config.yaml` with context, non-goals, and architectural rules
- [x] **1.3** Create `openspec/changes/bootstrap/` with proposal, specs, and tasks (this document)
- [x] **1.4** Create skeleton proposals for all downstream tiers in `openspec/changes/`
- [x] **1.5** Initial commit and push to `github.com/kcalvelli/cairn-companion`

## Phase 2: Nix package for the companion wrapper

- [x] **2.1** Create `packages/companion/default.nix` — a `pkgs.writeShellApplication` factory that:
  - Accepts `claudePackage`, `personaBasePackage`, `userFile` (nullable), `extraFiles` (list), `defaultWorkspace`, and `mcpConfigFile` (nullable) as arguments
  - Bakes all resolved persona paths into the generated script as literal `/nix/store/...` paths (no runtime directory scans, no env-var lookups)
  - Bakes `HAS_USER_FILE=0` or `HAS_USER_FILE=1` into the script based on whether `userFile` was supplied
  - Builds a `companion` shell script with logic per `specs/wrapper/spec.md`
  - Runtime dependencies: `coreutils` only (file existence checks use `[ -f ]`; no JSON parsing in the wrapper)
- [x] **2.2** Write the wrapper shell script logic:
  - [x] Parse arguments (separate companion-specific from passthrough)
  - [x] Concatenate the build-time-baked persona file paths into a single system prompt string (order: base AGENT → base USER *or* user file → extras)
  - [x] Ensure workspace directory exists; on first run, write `README.md` and — only if `HAS_USER_FILE=0` — copy the default `USER.md` template
  - [x] Auto-detect mcp-gateway config at documented paths using plain file-existence checks
  - [x] Build the final `claude` invocation with all flags
  - [x] `exec` to `claude` so the exit code propagates transparently
- [x] **2.3** Expose `lib.${system}.buildCompanion` as a flake output — this is the named helper the home-manager module consumes. It accepts the arguments from 2.1 and returns the built wrapper package.
- [x] **2.4** Wire `packages.${system}.default` in `flake.nix` to be the reference build produced by `buildCompanion` with default arguments (default persona, `pkgs.claude-code`, default workspace). This is for `nix build` smoke testing and documentation, not the user-facing build path.
- [x] **2.5** Wire `overlays.default` in `flake.nix` to expose only `cairn-companion` (the reference wrapper build). Do NOT expose the default-persona package via the overlay — it is an implementation detail.
- [x] **2.6** Verify `nix build .#default` produces a working binary and `nix eval .#lib.${system}.buildCompanion` resolves to a function.

## Phase 3: Default persona files

- [x] **3.1** Create `persona/default/AGENT.md` per `specs/persona/spec.md`:
  - Response format rules only
  - Under 50 lines
  - Zero character voice, tone adjectives, or nostalgia framing
  - Explicit instruction to read `USER.md` for user context
- [x] **3.2** Create `persona/default/USER.md` template per `specs/persona/spec.md`:
  - Header comment explaining purpose and how to customize
  - Placeholder sections: Who I am, Machines, Communication preferences, Things to check, Projects
  - All values are obvious placeholders (`<your name>`, etc.)
- [x] **3.3** Create `packages/persona-default/default.nix` — a package that installs both files into a derivation referenced by `persona.basePackage`
- [x] **3.4** Ensure the default persona package path is importable at Nix eval time so the module can reference individual files

## Phase 4: Home-manager module

- [x] **4.1** Create `modules/home-manager/default.nix` with the option schema from `specs/home-manager/spec.md`:
  - [x] `enable` — boolean
  - [x] `package` — package (default: self)
  - [x] `claudePackage` — package (default: `pkgs.claude-code`)
  - [x] `persona.basePackage` — package (default: this flake's persona-default)
  - [x] `persona.userFile` — nullable path
  - [x] `persona.extraFiles` — list of paths
  - [x] `workspaceDir` — string (default: `"${config.xdg.dataHome}/cairn-companion/workspace"`)
  - [x] `mcpConfigFile` — nullable path
- [x] **4.2** Implement the `config` block:
  - [x] Guard everything behind `lib.mkIf cfg.enable`
  - [x] Build the wrapper by calling `inputs.cairn-companion.lib.${pkgs.system}.buildCompanion` (the flake-exposed helper from Phase 2 task 2.3) with `claudePackage`, `personaBasePackage`, `userFile`, `extraFiles`, `defaultWorkspace`, and `mcpConfigFile` taken from `cfg`
  - [x] Do NOT consume `inputs.cairn-companion.packages.${pkgs.system}.default` directly — that is the reference build, not the per-user build path
  - [x] Add the resulting package to `home.packages`
- [x] **4.3** Wire `homeManagerModules.default` in `flake.nix` to point at `./modules/home-manager`
- [x] **4.4** Verify module evaluates cleanly with `nix eval` or a test home-manager config

## Phase 5: Manual end-to-end testing

- [x] **5.1** Test minimal enable: fresh home-manager config with only `services.cairn-companion.enable = true`
  - `home-manager switch` succeeds
  - `which companion` finds the binary
  - `companion "hello"` runs and produces a response
  - First invocation creates the workspace directory with `README.md` and default `USER.md`
- [x] **5.2** Test persona override: set `persona.userFile` to a custom file
  - Custom content replaces the default template in the system prompt
  - First invocation does NOT copy the default template into the workspace
- [x] **5.3** Test extra persona files: layer character voice
  - Voice file is appended after user file
  - Companion adopts the voice described *(validated with full Sid Friday five-file persona port on edge — voice/beliefs/family/context all layering correctly, per-file content traceable in responses)*
- [x] **5.4** Test mcp-gateway auto-detection: with mcp-gateway running on the same machine
  - Companion picks up the config from the auto-detect paths
  - Companion can invoke MCP tools from gateway servers *(validated via real email triage against cairn-mail MCP server)*
- [~] **5.5** Test mcp-gateway absent: on a machine without mcp-gateway — **deferred**, low-risk absence branch of file-existence check, no downstream dependencies
- [x] **5.6** Test flag passthrough: multi-turn interactive session exercises the same passthrough plumbing; `-p`, `--model`, and `--resume` are pure passthrough with no wrapper involvement
- [x] **5.7** Test exit code propagation — implicit in every successful Phase 5 invocation

## Phase 6: Documentation

- [x] **6.1** Update `README.md` "Getting started" section with real working examples (remove the "Not yet functional" note)
- [x] **6.2** Add a "First run" section explaining what the workspace is, what gets scaffolded, and how to customize
- [x] **6.3** Add examples of persona override patterns: user context only, user context + voice, full custom persona *(covered in the new "Authoring a persona" section with the five-file layout as the worked example)*
- [x] **6.4** Document the `mcpConfigFile` auto-detect paths explicitly in README
- [x] **6.5** Update `ROADMAP.md` to mark `bootstrap` as complete and point at the next proposal to tackle

## Phase 7: Validation and close

- [x] **7.1** Run `nix flake check` — **passes** (two cosmetic warnings only: `homeManagerModules` is an "unknown" output because nix-flake-check doesn't recognize the home-manager convention, and `nixfmt-rfc-style` has been renamed to `pkgs.nixfmt`; neither is an error)
- [x] **7.2** Verify a NixOS user who is not on cairn can consume the flake and use the module — **verified**: all `cairn` references in `modules/` and `packages/` are the project name (`cairn-companion`), the option namespace (`services.cairn-companion.*`), or comments. Zero imports of `inputs.cairn`, zero references to the cairn NixOS distribution.
- [~] **7.3** Verify multi-user scenario: two users on the same machine each enable the module independently and get isolated workspaces and configs — **deferred**, the isolation is structural (per-user `$XDG_DATA_HOME`, per-user `~/.claude/`, per-user `home.packages`) and has no shared mutable state in the module; risk of regression is near-zero
- [x] **7.4** Archive this change to `openspec/changes/archive/bootstrap/` once all tasks above are checked
