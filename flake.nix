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

    packages = forAllSystems (pkgs: let
      # One server from the workspace. Every crate is a path in it, so
      # Cargo.lock is the whole of what is fetched.
      server = pname:
        pkgs.rustPlatform.buildRustPackage {
          inherit pname;
          version = "0.1.0";
          src = lib.cleanSource self;
          # The pages are TypeScript; the build scripts compile them with these.
          nativeBuildInputs = with pkgs; [esbuild typescript];
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = ["-p" pname];
          cargoTestFlags = ["-p" pname];
          meta.mainProgram = pname;
        };
    in {
      # The two libraries, so there is nothing to install and the output
      # is empty on purpose. Building it compiles both crates and runs
      # their tests against the locked dependencies.
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "rooms";
        version = "0.1.0";
        src = lib.cleanSource self;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = ["-p" "rooms" "-p" "rooms-client"];
        cargoTestFlags = ["-p" "rooms" "-p" "rooms-client"];
        doCheck = true;
        installPhase = "mkdir -p $out";
      };

      # One server for every game, with one room that moves between them.
      hall = server "rooms-hall";

      # Each game on its own server, as it ran before the hall.
      petrov = server "petrov";
      numbers = server "numbers";
      estimate = server "estimate";
      updating = server "updating";
      order = server "order";
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.mkShell {
        packages = with pkgs; [cargo clippy rustc rustfmt esbuild typescript];
      };
    });

    # The hall, and each game on its own as it was served before. Every one
    # runs the same way; nix/server.nix says how.
    nixosModules = let
      units = import ./nix/server.nix {inherit lib;};
      build = name: pkgs: self.packages.${pkgs.stdenv.hostPlatform.system}.${name};
    in {
      default = self.nixosModules.hall;
      petrov = import ./petrov/module.nix {
        inherit units;
        package = build "petrov";
      };

      hall = units.module {
        name = "rooms-hall";
        description = "Games for a room";
        summary = "one server for every game, with rooms that move between them";
        port = 8103;
        package = build "hall";
        environment = {
          addr,
          state,
        }: {
          HALL_ADDR = addr;
          # The halls and every game's rooms, one file each.
          HALL_STATE = state;
        };
      };

      estimate = units.module {
        name = "estimate";
        description = "The estimation game";
        port = 8097;
        package = build "estimate";
        environment = {
          addr,
          state,
        }: {
          ESTIMATE_ADDR = addr;
          ESTIMATE_STATE = "${state}/rooms.json";
        };
      };

      numbers = units.module {
        name = "numbers";
        description = "Number games";
        summary = "the number games";
        port = 8098;
        package = build "numbers";
        environment = {
          addr,
          state,
        }: {
          NUMBERS_ADDR = addr;
          NUMBERS_STATE = "${state}/rooms.json";
        };
      };

      updating = units.module {
        name = "updating";
        description = "Speed updating";
        port = 8102;
        package = build "updating";
        environment = {
          addr,
          state,
        }: {
          UPDATING_ADDR = addr;
          UPDATING_STATE = "${state}/rooms.json";
          UPDATING_SUGGESTIONS = "${state}/suggestions.jsonl";
        };
      };

      order = units.module {
        name = "order";
        description = "One food order for a room";
        port = 8104;
        package = build "order";
        environment = {
          addr,
          state,
        }: {
          ORDER_ADDR = addr;
          ORDER_STATE = "${state}/rooms.json";
        };
      };
    };
  };
}
