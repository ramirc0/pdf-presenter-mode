{
  description = "Presenter console for PDF slide decks";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    nixgl = {
      url = "github:nix-community/nixGL";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, nixgl, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      nixGLIntel = nixgl.packages.${system}.nixGLIntel;
      # winit and glutin dlopen these at run time, and rfd dlopens libdbus for the file chooser.
      runtimeLibs = with pkgs; [
        dbus
        libGL
        libxkbcommon
        wayland
      ];
      cargoToml = pkgs.lib.importTOML ./Cargo.toml;
    in
    {
      # GL needs the host's driver: wrap with nixGL outside NixOS.
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = cargoToml.package.name;
        inherit (cargoToml.package) version;
        src = pkgs.lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;
        # The binary embeds ProggyForever, whose MIT license must travel with it.
        postInstall = ''
          install -Dm644 LICENSE assets/fonts/ProggyForever-LICENSE.txt \
            -t $out/share/licenses/${cargoToml.package.name}
        '';
        # Fixup shrinks RUNPATH to linked libraries, so add the dlopened ones after it.
        postFixup = ''
          patchelf --add-rpath ${pkgs.lib.makeLibraryPath runtimeLibs} $out/bin/ppm
        '';
        meta = {
          inherit (cargoToml.package) description;
          license = pkgs.lib.licenses.mit;
          mainProgram = "ppm";
          platforms = [ system ];
        };
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          cargo
          clippy
          grim
          jq
          just
          nixGLIntel
          pkg-config
          rust-analyzer
          rustc
          rustfmt
          sway
          typst
          wtype
        ];
        buildInputs = runtimeLibs;
        # Bake the dlopened libraries into RUNPATH, so builds run outside this shell.
        RUSTFLAGS = "-C link-arg=-Wl,-rpath,${pkgs.lib.makeLibraryPath runtimeLibs}";
        # The binary links Nix glibc, so it needs nixGL to load the host's GL driver.
        CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER = "${nixGLIntel}/bin/nixGLIntel";
      };
    };
}
