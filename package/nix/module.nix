{ defaultPackage }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.we-layerd;
in
{
  options.services.we-layerd = {
    enable = lib.mkEnableOption "we-layerd wallpaper daemon";

    package = lib.mkOption {
      type = lib.types.package;
      default = defaultPackage;
      defaultText = lib.literalExpression "inputs.we-layerd.packages.x86_64-linux.default";
      description = "The we-layerd package to install.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = pkgs.stdenv.hostPlatform.system == "x86_64-linux";
        message = "services.we-layerd currently supports x86_64-linux only.";
      }
    ];

    environment.systemPackages = [ cfg.package ];

    systemd.user.services.we-layerd = {
      description = "we-layerd wallpaper daemon";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session-pre.target" ];
      unitConfig.ConditionPathExists = "%h/.config/we-layerd/config.toml";
      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/we-layerd run --config %h/.config/we-layerd/config.toml";
        Restart = "on-failure";
        RestartSec = 2;
      };
    };
  };
}
