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

    # The hall, and each game on its own as it was served before.
    nixosModules.default = self.nixosModules.hall;
    nixosModules.petrov = import ./petrov/module.nix self.packages;
    nixosModules.numbers = import ./numbers/module.nix self.packages;
    nixosModules.estimate = import ./estimate/module.nix self.packages;
    nixosModules.updating = import ./updating/module.nix self.packages;

    nixosModules.hall = {
      config,
      lib,
      pkgs,
      ...
    }: let
      cfg = config.services.rooms-hall;
    in {
      options.services.rooms-hall = {
        enable = lib.mkEnableOption "one server for every game, with rooms that move between them";
        package = lib.mkOption {
          type = lib.types.package;
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.hall;
        };
        port = lib.mkOption {
          type = lib.types.port;
          default = 8103;
        };
        domains = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [];
          description = "Serve through nginx with ACME on each of these domains.";
        };
      };

      config = lib.mkIf cfg.enable {
        # systemd holds the port and passes it in, so the server itself
        # needs no network, as with each game on its own.
        systemd.sockets.rooms-hall = {
          description = "Games for a room socket";
          wantedBy = ["sockets.target"];
          listenStreams = ["127.0.0.1:${toString cfg.port}"];
        };

        systemd.services.rooms-hall = {
          description = "Games for a room";
          wantedBy = ["multi-user.target"];
          requires = ["rooms-hall.socket"];
          after = ["rooms-hall.socket"];
          environment = {
            HALL_ADDR = "127.0.0.1:${toString cfg.port}";
            # The halls and every game's rooms, one file each.
            HALL_STATE = "/var/lib/rooms-hall";
          };
          serviceConfig = {
            ExecStart = lib.getExe cfg.package;
            DynamicUser = true;
            StateDirectory = "rooms-hall";
            Restart = "always";
            RestartSec = 1;

            PrivateNetwork = true;
            IPAddressDeny = "any";
            RestrictAddressFamilies = "none";
            TemporaryFileSystem = "/var/lib:ro";
            InaccessiblePaths = ["-/mnt" "-/media" "-/srv"];
            ProtectHome = true;
            ProtectSystem = "strict";
            PrivateTmp = true;
            PrivateDevices = true;
            DevicePolicy = "closed";
            PrivateUsers = true;
            PrivateIPC = true;
            ProtectProc = "invisible";
            ProcSubset = "pid";
            ProtectClock = true;
            ProtectHostname = true;
            ProtectKernelLogs = true;
            ProtectKernelModules = true;
            ProtectKernelTunables = true;
            ProtectControlGroups = true;
            CapabilityBoundingSet = "";
            NoNewPrivileges = true;
            RestrictNamespaces = true;
            RestrictRealtime = true;
            RestrictSUIDSGID = true;
            LockPersonality = true;
            MemoryDenyWriteExecute = true;
            RemoveIPC = true;
            SystemCallArchitectures = "native";
            SystemCallFilter = ["@system-service" "~@privileged @resources"];
            SystemCallErrorNumber = "EPERM";
            UMask = "0077";
          };
        };

        services.nginx.virtualHosts = lib.genAttrs cfg.domains (domain: {
          forceSSL = true;
          enableACME = true;
          locations."/" = {
            proxyPass = "http://127.0.0.1:${toString cfg.port}";
            # Event streams: nothing to buffer and nothing to time out.
            extraConfig = ''
              proxy_buffering off;
              proxy_read_timeout 1h;
            '';
          };
        });
      };
    };
  };
}
