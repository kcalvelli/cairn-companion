# Default persona package for axios-companion Tier 0.
#
# Ships two files into $out:
#   - AGENT.md : response format rules, zero character voice
#   - USER.md  : template with placeholders the user fills in
#
# Consumed by packages/companion/default.nix as `personaBasePackage`. The
# companion wrapper bakes literal store paths to these files into its
# generated shell script — see specs/wrapper/spec.md "Persona Paths Are
# Resolved At Build Time".
{ stdenvNoCC, lib }:
stdenvNoCC.mkDerivation {
  pname = "axios-companion-persona-default";
  version = "0.1.0";

  src = ../../persona/default;

  dontBuild = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out
    install -Dm644 AGENT.md $out/AGENT.md
    install -Dm644 USER.md $out/USER.md
    runHook postInstall
  '';

  meta = {
    description = "Character-free default persona files for axios-companion";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
