# The numbers service. The flake at the top of the workspace passes in its
# packages, so the module serves this game's build by default.
packages: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.services.numbers;
in {
  options.services.numbers = {
    enable = lib.mkEnableOption "the number games";
    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${pkgs.stdenv.hostPlatform.system}.numbers;
    };
    port = lib.mkOption {
      type = lib.types.port;
      default = 8098;
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
    systemd.sockets.numbers = {
      description = "Number games socket";
      wantedBy = ["sockets.target"];
      listenStreams = ["127.0.0.1:${toString cfg.port}"];
    };

    systemd.services.numbers = {
      description = "Number games";
      wantedBy = ["multi-user.target"];
      requires = ["numbers.socket"];
      after = ["numbers.socket"];
      environment = {
        NUMBERS_ADDR = "127.0.0.1:${toString cfg.port}";
        NUMBERS_STATE = "/var/lib/numbers/rooms.json";
      };
      serviceConfig = {
        ExecStart = lib.getExe cfg.package;
        DynamicUser = true;
        StateDirectory = "numbers";
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
}
