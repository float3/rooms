# The petrov service. The flake at the top of the workspace passes in its
# packages, so the module serves this game's build by default.
packages: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.petrov;
  site = lib.types.submodule {
    options = {
      brand = lib.mkOption {
        type = lib.types.enum ["petrov" "arkhipov"];
        default = "petrov";
      };
      port = lib.mkOption {
        type = lib.types.port;
      };
      domains = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = "Serve through nginx with ACME on each of these domains.";
      };
    };
  };
in {
  options.services.petrov = {
    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${pkgs.stdenv.hostPlatform.system}.petrov;
    };
    sites = lib.mkOption {
      type = lib.types.attrsOf site;
      default = {};
      description = "Each site is its own service with its state in /var/lib/<name>.";
    };
  };

  config = {
    # systemd holds the port and passes it in, so the server itself needs
    # no network: nginx reaches it through the socket, and it cannot reach
    # anything, not even the other services listening on loopback.
    systemd.sockets =
      lib.mapAttrs (name: site: {
        description = "${site.brand} ritual server socket (${name})";
        wantedBy = ["sockets.target"];
        listenStreams = ["127.0.0.1:${toString site.port}"];
      })
      cfg.sites;

    systemd.services =
      lib.mapAttrs (name: site: {
        description = "${site.brand} ritual server (${name})";
        wantedBy = ["multi-user.target"];
        requires = ["${name}.socket"];
        after = ["${name}.socket"];
        environment = {
          PETROV_ADDR = "127.0.0.1:${toString site.port}";
          PETROV_BRAND = site.brand;
          PETROV_STATE = "/var/lib/${name}/games.json";
        };
        serviceConfig = {
          ExecStart = lib.getExe cfg.package;
          DynamicUser = true;
          StateDirectory = name;
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
      })
      cfg.sites;

    services.nginx.virtualHosts = lib.mkMerge (lib.mapAttrsToList (name: site:
      lib.genAttrs site.domains (domain: {
        forceSSL = true;
        enableACME = true;
        locations."/".proxyPass = "http://127.0.0.1:${toString site.port}";
      }))
    cfg.sites);
  };
}
