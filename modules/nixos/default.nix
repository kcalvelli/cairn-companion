# NixOS module for cairn-companion system-level concerns.
# Currently: Syncthing-based memory sync between machines.
{
  config,
  lib,
  ...
}:

let
  cfg = config.services.cairn-companion.sync;

  # Compute the Claude Code project memory path from the workspace path.
  # Claude Code slugifies by replacing `/` and `.` with `-`.
  userHome = "/home/${cfg.user}";
  workspacePath = "${userHome}/.local/share/cairn-companion/workspace";
  slug = builtins.replaceStrings [ "/" "." ] [ "-" "-" ] workspacePath;
  memoryPath = "${userHome}/.claude/projects/${slug}/memory";
in
{
  options.services.cairn-companion.sync = {
    enable = lib.mkEnableOption "Syncthing-based memory sync for cairn-companion";

    user = lib.mkOption {
      type = lib.types.str;
      description = "User whose companion memory should be synced.";
      example = "keith";
    };

    devices = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = ''
        Syncthing device names to share companion memory with.
        These must match device names already configured in your
        Syncthing settings (e.g., via cairn.syncthing.devices).
      '';
      example = [ "mini" "edge" ];
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = config.services.syncthing.enable;
        message = ''
          services.cairn-companion.sync requires Syncthing to be enabled.
          Set services.syncthing.enable = true or use cairn.syncthing.
        '';
      }
    ];

    services.syncthing.settings.folders."companion-memory" = {
      path = memoryPath;
      devices = cfg.devices;
      ignorePerms = false;
    };
  };
}
