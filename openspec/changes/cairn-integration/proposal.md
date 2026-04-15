# Proposal: Cairn Integration — Consumer-Side Wiring

> **Status**: Skeleton — this proposal is a roadmap placeholder. Unlike other proposals in this repo, the actual change artifact for this one will live in the [cairn](https://github.com/kcalvelli/cairn) repository under `cairn/openspec/changes/`, not here. This placeholder exists so the roadmap is complete from cairn-companion's perspective.

## Tier

Consumer-side integration (not a tier of cairn-companion itself)

## Summary

Add `cairn-companion` as a flake input to cairn, import the home-manager module into cairn's home profile, and expose companion-related options under cairn's own module namespace so cairn users get sensible defaults for their environment (Niri-aware spoke tools, DMS-aware notifications, agenix-backed secret file paths, default MCP gateway integration).

## Motivation

cairn-companion is designed to work on any NixOS system, but cairn users should get the best out-of-the-box experience because cairn is the canonical environment this project is designed for. That means: the companion should be enableable with a single option in an cairn user's home config, with defaults that match cairn's conventions (agenix secret paths, mcp-gateway integration, Niri/DMS-aware tool selection).

This is a thin integration layer. It does not modify the cairn-companion repository — it only adds consumption glue to cairn. Publishing this as a separate proposal keeps cairn's SDD history honest about what changed on the cairn side when cairn-companion landed.

## Scope

### In scope (lives in cairn repo)

- Add `cairn-companion` as a flake input in `cairn/flake.nix`
- Create `cairn/home/ai/companion.nix` that imports `inputs.cairn-companion.homeManagerModules.default`
- Add `services.cairn.companion.enable` option (cairn-side wrapper) that:
  - Enables `services.cairn-companion.enable` with defaults suitable for cairn users
  - Wires agenix secret file paths for channel credentials (telegram, email, discord, xmpp)
  - Points `mcpConfigFile` at cairn's known mcp-gateway output location
  - When Niri + DMS is detected, enables the Niri-aware spoke tools automatically
- Update cairn's `home/ai/default.nix` to include the companion module
- Add documentation in cairn README about enabling the companion

### Out of scope

- Modifying the `cairn-companion` repository — this is purely an cairn-side change
- Forking or patching `cairn-companion` — all changes go upstream via PRs to `cairn-companion`
- Custom cairn-specific tool servers — if a tool is useful for cairn users, it should ship in `cairn-companion` and be opt-in via module options, not cairn-specific

### Non-goals

- Making `cairn-companion` depend on cairn (the whole point of the separate-repo design is the opposite)
- Replacing cairn users' manual configuration — cairn users can still override any option

## Dependencies

- `bootstrap` must be shipped (the module must exist to be imported)
- Ideally also `daemon-core` and `cli-client` so cairn users get the full CLI experience on first enable — but the cairn integration can land with just Tier 0 and add Tier 1 later

## Success criteria

1. An cairn user can add `services.cairn.companion.enable = true;` to their home configuration, run `home-manager switch`, and get a working companion with sensible cairn defaults
2. cairn's agenix secrets (telegram bot token, email password, etc.) are automatically wired when the corresponding channels are enabled
3. cairn users on Niri + DMS automatically get Wayland-native spoke tools without configuring them individually
4. Non-cairn users consuming `cairn-companion` directly continue to work unchanged — this proposal adds nothing to the companion repo itself
5. The change is documented in both cairn and cairn-companion README files, with the cairn side being the primary documentation
