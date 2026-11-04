{
  description = "Speed updating for a room of people";

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
      default = pkgs.rustPlatform.buildRustPackage {
        pname = "updating";
        version = "0.1.0";
        src = lib.cleanSource self;
        # The page is TypeScript; the build script compiles it with these.
        nativeBuildInputs = with pkgs; [esbuild typescript];
        cargoLock = {
          lockFile = ./Cargo.lock;
          # rooms is a git dependency, so nix needs its hash as well as
          # cargo's. Both change together: after bumping the rev in
          # Cargo.lock, run
          #   nix run nixpkgs#nix-prefetch-git -- --url https://github.com/float3/rooms --rev <rev>
          # and put the hash it prints here.
          outputHashes."rooms-0.1.0" = "sha256-zFKbyYQiRP5R+3HDF2cGCvYG4Ff4QxNwDt0jLRRmfsA=";
          outputHashes."rooms-client-0.1.0" = "sha256-zFKbyYQiRP5R+3HDF2cGCvYG4Ff4QxNwDt0jLRRmfsA=";
        };
        meta.mainProgram = "updating";
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
      cfg = config.services.updating;
    in {
      options.services.updating = {
        enable = lib.mkEnableOption "speed updating";
        package = lib.mkOption {
          type = lib.types.package;
          default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        };
        port = lib.mkOption {
          type = lib.types.port;
          default = 8102;
        };
        domains = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [];
          description = "Serve through nginx with ACME on each of these domains.";
        };
      };

      config = lib.mkIf cfg.enable {
        # systemd holds the port and passes it in, so the server itself
        # needs no network: nginx reaches it through the socket, and it
        # cannot reach anything, not even the other services on loopback.
        systemd.sockets.updating = {
          description = "The speed updating socket";
          wantedBy = ["sockets.target"];
          listenStreams = ["127.0.0.1:${toString cfg.port}"];
        };

        systemd.services.updating = {
          description = "Speed updating";
          wantedBy = ["multi-user.target"];
          requires = ["updating.socket"];
          after = ["updating.socket"];
          environment = {
            UPDATING_ADDR = "127.0.0.1:${toString cfg.port}";
            UPDATING_STATE = "/var/lib/updating/rooms.json";
            UPDATING_SUGGESTIONS = "/var/lib/updating/suggestions.jsonl";
          };
          serviceConfig = {
            ExecStart = lib.getExe cfg.package;
            DynamicUser = true;
            StateDirectory = "updating";
            Restart = "always";
            RestartSec = 1;

            PrivateNetwork = true;
            IPAddressDeny = "any";
            # Serving the passed socket needs no new sockets of any kind.
            RestrictAddressFamilies = "none";
            # Only its own state directory under /var/lib; the rest is hidden.
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
