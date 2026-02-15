{
  description = "Nix flake for building the setrixtui Rust crate using naersk";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = inputs@{ self, flake-utils, nixpkgs, naersk, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };


        # naersk.buildPackage below.
        naerskPkg = pkgs.callPackage naersk {};
      in
      rec {
        packages = {
          # Primary package built from this crate.
          default = naerskPkg.buildPackage {
            name = "setrixtui";
            src = ./.;
            # Build a release binary by default
            release = true;
          };
        };

        devShells = {
          default = pkgs.mkShell {
            buildInputs = [
              pkgs.rustc
              pkgs.cargo
              pkgs.rustfmt
              pkgs.clippy
            ];

            shellHook = ''
              echo "Entering development shell for setrixtui"
              echo "Use: cargo build / cargo run or nix build .#"
            '';
          };
        };

        apps = {
          default = {
            type = "app";
            program = [ "${packages.default}/bin/setrixtui" ];
          };
        };
      }
    );
}
