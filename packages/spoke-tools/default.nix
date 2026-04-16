# companion-spoke-tools — machine-local MCP tool servers.
#
# One cargo package, multiple [[bin]] entries. Each binary is a
# short-lived stdio MCP server spawned per-call by mcp-gateway. This
# package builds all of them in one shot; the home-manager module
# picks which ones to register with mcp-gateway via the
# `services.cairn-companion.spoke.tools.<tool>.enable` toggles.
#
# Each tool's runtime dependencies (notify-send, grim, wl-clipboard,
# etc.) are added to the wrapped binary's PATH via makeWrapper so the
# tool works regardless of what PATH mcp-gateway's systemd unit
# happens to have.
{
  lib,
  rustPlatform,
  makeWrapper,
  libnotify,
  grim,
  wl-clipboard,
  systemd,
  xdg-utils,
  dex,
  niri,
  # Shell-tool inspection toolkit — see the `companion-mcp-shell`
  # wrapProgram below for the rationale.
  coreutils,
  git,
  gnugrep,
  ripgrep,
  procps,
  file,
  which,
}:
rustPlatform.buildRustPackage {
  pname = "companion-spoke-tools";
  version = "0.1.0";

  src = ./.;

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  # Per-tool shell-outs:
  #   libnotify    → notify-send             (notify)
  #   grim         → screen capture          (screenshot)
  #   wl-clipboard → wl-copy / wl-paste      (clipboard)
  #   systemd      → journalctl / systemctl  (journal)
  #   xdg-utils    → xdg-open                (apps: open_url)
  #   dex          → desktop-entry launcher  (apps: launch_desktop_entry)
  #   niri         → compositor IPC          (niri: all tools)
  # The `shell` tool bundles a small inspection toolkit (coreutils,
  # git, grep, ripgrep, procps, file, which) on its wrapped PATH.
  # mcp-gateway's service PATH is narrow and doesn't include most of
  # these, so without the bundle the allowlist and the reachable set
  # disagree: Keith allowlists `git`, Sid calls it, spawn fails with
  # ENOENT because git isn't resolvable. For commands outside this
  # bundle (nix, systemctl, etc.), the allowlist + the inherited
  # PATH handle it — those are already reachable through
  # /run/current-system/sw/bin or the service's own PATH entries.
  # Each tool's runtime PATH is wrapped below so the shell-out resolves
  # regardless of the mcp-gateway unit's inherited PATH.
  buildInputs = [
    libnotify
    grim
    wl-clipboard
    systemd
    xdg-utils
    dex
    niri
    coreutils
    git
    gnugrep
    ripgrep
    procps
    file
    which
  ];

  postInstall = ''
    wrapProgram $out/bin/companion-mcp-notify \
      --prefix PATH : ${lib.makeBinPath [ libnotify ]}

    wrapProgram $out/bin/companion-mcp-screenshot \
      --prefix PATH : ${lib.makeBinPath [ grim ]}

    wrapProgram $out/bin/companion-mcp-clipboard \
      --prefix PATH : ${lib.makeBinPath [ wl-clipboard ]}

    wrapProgram $out/bin/companion-mcp-journal \
      --prefix PATH : ${lib.makeBinPath [ systemd ]}

    wrapProgram $out/bin/companion-mcp-apps \
      --prefix PATH : ${lib.makeBinPath [ xdg-utils dex ]}

    wrapProgram $out/bin/companion-mcp-niri \
      --prefix PATH : ${lib.makeBinPath [ niri ]}

    wrapProgram $out/bin/companion-mcp-shell \
      --prefix PATH : ${lib.makeBinPath [
        coreutils git gnugrep ripgrep procps file which
      ]}
  '';

  meta = {
    description = "MCP tool servers exposing local-machine capabilities for cairn-companion";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
