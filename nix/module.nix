self:
{
  config,
  lib,
  pkgs,
  utils,
  ...
}:
let
  cfg = config.services.lyrica;
  inherit (lib) mkOption types;
in
{
  options.services.lyrica = {
    enable = lib.mkEnableOption "lyrica, a mobile-first PWA client for MPD";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "lyrica.packages.\${system}.default";
      description = "The lyrica package to run.";
    };

    settings = mkOption {
      description = ''
        Command-line arguments, by flag name (see `lyrica --help`): a string or
        number is passed as `--name value`; `null` omits the flag. Passwords
        belong in {option}`services.lyrica.environmentFile`.
      '';
      default = { };
      example = {
        listen = "127.0.0.1:8080";
        music-url = "https://music.example.com";
        log = "debug";
      };
      type = types.submodule {
        freeformType = with types; attrsOf (nullOr (either str int));
        options = {
          listen = mkOption {
            type = types.str;
            default = "127.0.0.1:8080";
            description = "Address to serve plain HTTP on, for a reverse proxy.";
          };
          mpd = mkOption {
            type = types.str;
            default = "127.0.0.1:6600";
            description = "MPD address (TCP only).";
          };
          music-url = mkOption {
            type = with types; nullOr str;
            default = null;
            description = "Base URL of an HTTP mirror of MPD's music directory; enables album art.";
          };
        };
      };
    };

    environmentFile = mkOption {
      type = with types; nullOr path;
      default = null;
      example = "/run/secrets/lyrica";
      description = ''
        File of `NAME=value` lines loaded into the service environment, kept out
        of the Nix store and the process list. Set `LYRICA_MPD_PASSWORD` here.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.lyrica = {
      description = "lyrica MPD web client";
      wantedBy = [ "multi-user.target" ];
      after = [
        "network.target"
        "mpd.service"
      ];
      serviceConfig = {
        ExecStart = utils.escapeSystemdExecArgs (
          [ (lib.getExe cfg.package) ] ++ lib.cli.toGNUCommandLine { } cfg.settings
        );
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
        Restart = "on-failure";

        # No state on disk and no privileges needed.
        DynamicUser = true;
        CapabilityBoundingSet = "";
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        PrivateDevices = true;
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
        UMask = "0077";
      };
    };
  };
}
