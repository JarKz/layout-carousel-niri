{
  description = "Layout carousel for niri WM";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils = {
      url = "github:numtide/flake-utils";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    {
      inherit (flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs {
            inherit system overlays;
          };

          rustToolchain = pkgs.rust-bin.stable.latest.minimal;

          lc-niri = pkgs.rustPlatform.buildRustPackage {
            pname = "lc-niri";
            version = "0.1.0";

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = [
              pkgs.pkg-config
            ];

            buildInputs = [ ];

            meta = with pkgs.lib; {
              description = "A layout carousel for niri WM";
              homepage = "https://github.com/jarkz/layout-carousel-niri";
              license = licenses.gpl3;
              maintainers = [ "jarkz" ];
              platforms = platforms.linux;
            };

            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        in
        {
          packages.default = lc-niri;
        }
      )) packages;
    };
}


