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
      # winit and glutin dlopen these at run time.
      runtimeLibs = with pkgs; [
        libGL
        libxkbcommon
        wayland
      ];
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          cargo
          clippy
          grim
          jq
          just
          nixgl.packages.${system}.nixGLIntel
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
      };
    };
}
