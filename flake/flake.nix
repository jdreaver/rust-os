{
  inputs = {
    nixpkgs-unstable.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs-unstable, rust-overlay }:
    let
      pkgs = import nixpkgs-unstable {
        system = "x86_64-linux";
        overlays = [ rust-overlay.overlays.default ];
        config = { allowUnfree = true; };
      };
    in
      with pkgs;
      {
      devShells.x86_64-linux.default = pkgs.mkShell {
        nativeBuildInputs = [
          (rust-bin.stable.latest.default.override {
            # extensions = [ "rust-src" ];
            targets = [ "thumbv7em-none-eabi" ];
          })

          # Rust
          # cargo
          # rustc
          # clippy
          # rustfmt
        ];
      };
    };
}
