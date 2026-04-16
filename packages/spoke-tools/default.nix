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
}:
rustPlatform.buildRustPackage {
  pname = "companion-spoke-tools";
  version = "0.1.0";

  src = ./.;

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  # Per-tool shell-outs — libnotify → notify-send, grim → screenshots.
  # Each tool's runtime PATH is wrapped below so the shell-out resolves
  # regardless of the mcp-gateway unit's inherited PATH.
  buildInputs = [
    libnotify
    grim
  ];

  postInstall = ''
    wrapProgram $out/bin/companion-mcp-notify \
      --prefix PATH : ${lib.makeBinPath [ libnotify ]}

    wrapProgram $out/bin/companion-mcp-screenshot \
      --prefix PATH : ${lib.makeBinPath [ grim ]}
  '';

  meta = {
    description = "MCP tool servers exposing local-machine capabilities for cairn-companion";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
