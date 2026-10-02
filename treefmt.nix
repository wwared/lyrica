{
  projectRootFile = "flake.nix";

  programs = {
    nixfmt.enable = true;
    rustfmt.enable = true;
    taplo.enable = true;
    prettier.enable = true;
  };

  settings.global.excludes = [
    "*.lock"
    "web/package-lock.json"
    ".envrc"
  ];
}
