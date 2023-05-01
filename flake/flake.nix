{
  inputs = {
    nixpkgs-unstable.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs-unstable, rust-overlay }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs-unstable {
        inherit system;
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
            ];
          }))

          # For emulation
          qemu

          # Build
          xorriso
        ];
      };

      packages.${system}.limine =
        let
          # See https://github.com/limine-bootloader/limine/releases for
          # releases. Make sure to use the "-binary" version!
          version = "v4.20230428.0-binary";
        in pkgs.stdenv.mkDerivation {
          pname = "limine";
          inherit version;

          src = pkgs.fetchFromGitHub {
            owner = "limine-bootloader";
            repo = "limine";
            rev = version;
            sha256 = "sha256-QnmKKRzcjDIDNO6YbbBpyFS09imdhYw046miFkQ1/Rw=";
          };

          buildPhase = ''
            make
          '';

          installPhase = ''
            cp -r . $out/
          '';
        };
      };
}
