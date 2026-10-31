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

    packages = forAllSystems (pkgs: {
      # The two libraries, so there is nothing to install and the output
      # is empty on purpose. Building it compiles both crates and runs
      # their tests against the locked dependencies, which is what the
      # tools that depend on them want to know.
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "rooms";
        version = "0.1.0";
        src = lib.cleanSource self;
        # The games the hall serves are git dependencies, pinned by rev in
        # Cargo.lock, so no hashes need to be kept here.
        cargoLock = {
          lockFile = ./Cargo.lock;
          allowBuiltinFetchGit = true;
        };
        cargoBuildFlags = ["-p" "rooms" "-p" "rooms-client"];
        cargoTestFlags = ["-p" "rooms" "-p" "rooms-client"];
        doCheck = true;
        installPhase = "mkdir -p $out";
      };

      # One server for every game, with one room that moves between them.
      hall = pkgs.rustPlatform.buildRustPackage {
        pname = "rooms-hall";
        version = "0.1.0";
        src = lib.cleanSource self;
        nativeBuildInputs = with pkgs; [esbuild typescript];
        cargoLock = {
          lockFile = ./Cargo.lock;
          allowBuiltinFetchGit = true;
        };
        cargoBuildFlags = ["-p" "rooms-hall"];
        cargoTestFlags = ["-p" "rooms-hall"];
        meta.mainProgram = "rooms-hall";
      };
    });

    devShells = forAllSystems (pkgs: {
      default = pkgs.mkShell {
        packages = with pkgs; [cargo clippy rustc rustfmt esbuild typescript];
      };
    });
  
    nixosModules.default = {
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
