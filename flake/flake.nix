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

      # Compile QEMU with debug symbols so we can run QEMU itself with GDB.
      qemu-x86_64 = pkgs.qemu.override {
        # We only need x86_64. This keeps compile times down.
        hostCpuOnly = true;

        # Wrapping the qemu-system-* binaries with the GTK wrapper in nixpkgs
        # removes debug symbols.
        gtkSupport = false;
      };
      qemu-x86_64-debug = qemu-x86_64.overrideAttrs (finalAttrs: previousAttrs: {
        # QEMU-specific flags to add debug info. See https://www.cnblogs.com/root-wang/p/8005212.html
        configureFlags = previousAttrs.configureFlags ++ [
          "--enable-debug"
          "--extra-cflags=-g3" # --enable-debug uses -g, we want even more
          "--disable-pie"
        ];

        # Disable default hardening flags. These are very confusing when doing
        # development and they break builds of packages/systems that don't
        # expect these flags to be on. Automatically enables stuff like
        # FORTIFY_SOURCE, -Werror=format-security, -fPIE, etc. See:
        # - https://nixos.org/manual/nixpkgs/stable/#sec-hardening-in-nixpkgs
        # - https://nixos.wiki/wiki/C#Hardening_flags
        hardeningDisable = ["all"];

        # Don't strip debug info from executables.
        dontStrip = true;

        # By default some script goes and separates debug info from the
        # binaries. We don't want that.
        separateDebugInfo = false;

        # Store all of the source artifacts so GDB can use them.
        #
        # Note that gdb expects us to be in the build/ sub-directory, and some
        # paths are still absolute. See
        # https://github.com/mesonbuild/meson/issues/10533 for possible
        # alternatives like -fdebug-prefix-map. Also see
        # https://alex.dzyoba.com/blog/gdb-source-path/
        postFixup = (previousAttrs.postFixup or "") + ''
          mkdir -p $out/raw
          # In Meson we are in a build/ subdirectory
          cp -r .. $out/raw/
        '';
      });
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
          parted
        ];
      };

      packages.${system} = {
        inherit qemu-x86_64-debug;

        # Nix has an OVMF package, but it doesn't seem to include OVMF.fd. We
        # use the zip file that the limine barebones build uses
        # https://github.com/limine-bootloader/limine-barebones/blob/e08f355a22fbefb27cfea4e3d890eb9551bdac1b/GNUmakefile#L28-L30
        OVMF = pkgs.stdenv.mkDerivation {
          name = "OVMF";
          src = pkgs.fetchzip {
            url = "https://efi.akeo.ie/OVMF/OVMF-X64.zip";
            sha256 = "sha256-dF+HQJ9TREfqxnUSAHWzkbkw93ifLIqmROhv3uM4Rss=";
            stripRoot = false;
          };

          installPhase = ''
            cp -r . $out/
          '';
        };

        limine =
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
      };
}
