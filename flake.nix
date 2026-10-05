{
  description = "lyrica: a mobile-first PWA client for MPD";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      treefmt-nix,
    }:
    let
      inherit (nixpkgs) lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      eachSystem = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      perSystem =
        pkgs:
        let
          craneLib = crane.mkLib pkgs;
          treefmt = treefmt-nix.lib.evalModule pkgs ./treefmt.nix;

          # Upstream webfont release: nixpkgs only packages TTF/TTC builds of Iosevka.
          iosevkaAile = pkgs.fetchzip {
            url = "https://github.com/be5invis/Iosevka/releases/download/v34.9.0/PkgWebFont-IosevkaAile-34.9.0.zip";
            hash = "sha256-G0WtYPxoWQQi4H00xwCYfEbb2bWQvZrB8Rp7HVGiH04=";
            stripRoot = false;
          };
          # The weights the UI uses; Vite bundles them from web/src/fonts.
          fonts = pkgs.runCommand "lyrica-fonts" { } ''
            mkdir $out
            cp ${iosevkaAile}/WOFF2/IosevkaAile-{Regular,SemiBold}.woff2 $out/
          '';

          webArgs = {
            version = (lib.importJSON ./web/package.json).version;
            src = ./web;
            npmDeps = pkgs.importNpmLock { npmRoot = ./web; };
            inherit (pkgs.importNpmLock) npmConfigHook;
          };
          web = pkgs.buildNpmPackage (
            webArgs
            // {
              pname = "lyrica-web";
              preBuild = "cp -r --no-preserve=mode ${fonts} src/fonts";
              installPhase = "cp -r dist $out";
            }
          );
          webTests = pkgs.buildNpmPackage (
            webArgs
            // {
              pname = "lyrica-web-tests";
              buildPhase = "npm run typecheck && npm test";
              installPhase = "touch $out";
            }
          );

          commonArgs = {
            pname = "lyrica";
            version = (lib.importTOML ./Cargo.toml).workspace.package.version;
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
          };
          # Excludes the frontend so web changes don't rebuild dependencies.
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          crateArgs = commonArgs // {
            inherit cargoArtifacts;
            LYRICA_WEB_DIST = web;
          };

          lyrica = craneLib.buildPackage (
            crateArgs
            // {
              cargoExtraArgs = "--locked -p lyrica";
              doCheck = false; # tests run in checks.cargo-test
              meta = {
                mainProgram = "lyrica";
                license = lib.licenses.unlicense;
              };
            }
          );
        in
        {
          packages = {
            default = lyrica;
            inherit lyrica web;
          };

          checks = {
            inherit lyrica;
            web-tests = webTests;
            cargo-clippy = craneLib.cargoClippy (
              crateArgs // { cargoClippyExtraArgs = "--all-targets -- --deny warnings"; }
            );
            cargo-test = craneLib.cargoTest crateArgs;
            formatting = treefmt.config.build.check self;
          };

          formatter = treefmt.config.build.wrapper;

          devShells.default = craneLib.devShell {
            packages = [
              pkgs.rust-analyzer
              pkgs.nodejs
              pkgs.mpc
              treefmt.config.build.wrapper
            ];
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            # Fresh copy on every entry so it tracks the flake; gitignored.
            shellHook = ''
              if root=$(git rev-parse --show-toplevel 2>/dev/null) && [ -d "$root/web/src" ]; then
                rm -rf "$root/web/src/fonts"
                cp -r --no-preserve=mode ${fonts} "$root/web/src/fonts"
              fi
            '';
          };
        };

      outputs = eachSystem perSystem;
    in
    {
      packages = lib.mapAttrs (_: o: o.packages) outputs;
      checks = lib.mapAttrs (_: o: o.checks) outputs;
      formatter = lib.mapAttrs (_: o: o.formatter) outputs;
      devShells = lib.mapAttrs (_: o: o.devShells) outputs;
      nixosModules.default = import ./nix/module.nix self;
    };
}
