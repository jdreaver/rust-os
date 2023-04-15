{ system ? builtins.currentSystem }:

# Flake is in a subdirectory so we don't copy this entire repo to /nix/store.
# See:
# - https://github.com/NixOS/nix/issues/3121
# - https://discourse.nixos.org/t/tweag-nix-dev-update-31/19481
# - https://github.com/NixOS/nix/issues/4097
(builtins.getFlake (toString ./flake)).devShells.${system}.default
