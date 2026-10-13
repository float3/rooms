{
  description = "A room other people join from their phones";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = {
    self,
    nixpkgs,
  }: let
    lib = nixpkgs.lib;
    systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
  in {
    formatter = forAllSystems (pkgs: pkgs.alejandra);

    # Two libraries, so there is nothing to install and the output is empty
    # on purpose. Building it compiles both crates and runs their tests
    # against the locked dependencies, which is what the tools that depend
    # on them want to know.
    packages = forAllSystems (pkgs: {
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "rooms";
        version = "0.1.0";
        src = lib.cleanSource self;
        cargoLock.lockFile = ./Cargo.lock;
        doCheck = true;
        installPhase = "mkdir -p $out";
      };
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.mkShell {
        packages = with pkgs; [cargo clippy rustc rustfmt esbuild typescript];
      };
    });
  };
}
