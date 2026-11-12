# What every server in this workspace runs as: systemd holds the port and
# passes it in, so the server itself needs no network. nginx reaches it
# through the socket, and it cannot reach anything, not even the other
# services on loopback. Its only writable place is its own state directory.
{lib}: rec {
  socket = {
    description,
    port,
  }: {
    inherit description;
    wantedBy = ["sockets.target"];
    listenStreams = ["127.0.0.1:${toString port}"];
  };

  service = {
    name,
    description,
    package,
    environment,
  }: {
    inherit description environment;
    wantedBy = ["multi-user.target"];
    requires = ["${name}.socket"];
    after = ["${name}.socket"];
    serviceConfig = {
      ExecStart = lib.getExe package;
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
  };

  # An nginx site with ACME on each domain. `streams` is for a server that
  # holds event streams open: nothing to buffer and nothing to time out.
  vhosts = {
    port,
    domains,
    streams ? true,
  }:
    lib.genAttrs domains (domain: {
      forceSSL = true;
      enableACME = true;
      locations."/" =
        {
          proxyPass = "http://127.0.0.1:${toString port}";
        }
        // lib.optionalAttrs streams {
          extraConfig = ''
            proxy_buffering off;
            proxy_read_timeout 1h;
          '';
        };
    });

  # A module for a server that runs once: services.<name> with enable,
  # package, port and domains. `package` picks the default build for the
  # host's pkgs; `environment` gets the address to serve on and the state
  # directory, /var/lib/<name>.
  module = {
    name,
    description,
    summary ? lib.toLower (lib.substring 0 1 description) + lib.substring 1 (-1) description,
    port,
    package,
    environment,
  }: {
    config,
    pkgs,
    ...
  }: let
    cfg = config.services.${name};
  in {
    options.services.${name} = {
      enable = lib.mkEnableOption summary;
      package = lib.mkOption {
        type = lib.types.package;
        default = package pkgs;
      };
      port = lib.mkOption {
        type = lib.types.port;
        default = port;
      };
      domains = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [];
        description = "Serve through nginx with ACME on each of these domains.";
      };
    };

    config = lib.mkIf cfg.enable {
      systemd.sockets.${name} = socket {
        description = "${description} socket";
        inherit (cfg) port;
      };
      systemd.services.${name} = service {
        inherit name description;
        inherit (cfg) package;
        environment = environment {
          addr = "127.0.0.1:${toString cfg.port}";
          state = "/var/lib/${name}";
        };
      };
      services.nginx.virtualHosts = vhosts {inherit (cfg) port domains;};
    };
  };
}
