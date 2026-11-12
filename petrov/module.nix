# The petrov service: one server per site, each with its own brand, port
# and state. The flake passes in the shared units and this game's build.
{
  units,
  package,
}: {
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
      default = package pkgs;
    };
    sites = lib.mkOption {
      type = lib.types.attrsOf site;
      default = {};
      description = "Each site is its own service with its state in /var/lib/<name>.";
    };
  };

  config = {
    systemd.sockets = lib.mapAttrs (name: site:
      units.socket {
        description = "${site.brand} ritual server socket (${name})";
        inherit (site) port;
      })
    cfg.sites;

    systemd.services = lib.mapAttrs (name: site:
      units.service {
        inherit name;
        description = "${site.brand} ritual server (${name})";
        inherit (cfg) package;
        environment = {
          PETROV_ADDR = "127.0.0.1:${toString site.port}";
          PETROV_BRAND = site.brand;
          PETROV_STATE = "/var/lib/${name}/games.json";
        };
      })
    cfg.sites;

    # nginx in front of petrov has always buffered; keep it that way.
    services.nginx.virtualHosts = lib.mkMerge (lib.mapAttrsToList (name: site:
      units.vhosts {
        inherit (site) port domains;
        streams = false;
      })
    cfg.sites);
  };
}
