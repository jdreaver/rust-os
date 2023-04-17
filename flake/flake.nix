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
          (rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
            extensions = [
              "rust-src" # Needed to rebuild core with build-std. See https://doc.rust-lang.org/cargo/reference/unstable.html#build-std
              "llvm-tools-preview"
            ];
            targets = [
              "x86_64-unknown-none"
              "thumbv7em-none-eabi" # TODO: deleteme?
            ];
          }))

          # For emulation
          qemu

        ];
      };
    };
}
