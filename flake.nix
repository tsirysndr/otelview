{
  description = "otelview — open-source, self-hosted OpenTelemetry viewer in a single binary";

  nixConfig = {
    extra-substituters = [ "https://otelview.cachix.org" ];
    extra-trusted-public-keys = [
      "otelview.cachix.org-1:+Twrf64f2rg+cTAYU2MikV/hGMpHxnHK6l6yLvrseP4="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, crane }:
    let
      systems = [ "aarch64-darwin" "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system:
        f (import nixpkgs { inherit system; }));

      duckdbVersion = "v1.5.5";
      duckdbAssets = {
        aarch64-darwin = {
          name = "static-libs-osx-arm64.zip";
          hash = "sha256-157Ga4pAVLhm+q2oLp4x+FmnE8VVs/HEtxxKQ9MnPpw=";
        };
        x86_64-linux = {
          name = "static-libs-linux-amd64.zip";
          hash = "sha256-3rR8UwDzyZcl6EzbFNIUw7ErvXSLYTsWmLk4yJTLaOs=";
        };
        aarch64-linux = {
          name = "static-libs-linux-arm64.zip";
          hash = "sha256-6mo0y0nsLbXtI9noMRI3xTwyq/nNv13WCMQXbD3Yv+s=";
        };
      };
    in
    {
      packages = forAllSystems (pkgs:
        let
          inherit (pkgs) lib;
          craneLib = crane.mkLib pkgs;
          asset = duckdbAssets.${pkgs.stdenv.hostPlatform.system};

          # Pre-built static DuckDB from the official GitHub release —
          # never compiled locally, matching scripts/fetch-duckdb.sh.
          duckdb-static = pkgs.runCommand "duckdb-static-${duckdbVersion}"
            {
              src = pkgs.fetchurl {
                url = "https://github.com/duckdb/duckdb/releases/download/${duckdbVersion}/${asset.name}";
                inherit (asset) hash;
              };
              nativeBuildInputs = [ pkgs.unzip ];
            } ''
            mkdir -p $out
            unzip -q $src -d $out
          '';

          # Web UI built with bun. Fixed-output derivation: network access
          # for `bun install` is allowed, the dist output is hash-pinned.
          # After UI changes, refresh the hash: run the nix workflow (or
          # `nix build .#webui`) and paste the "got:" hash from the mismatch
          # error.
          webui = pkgs.stdenvNoCC.mkDerivation {
            pname = "otelview-webui";
            version = "0.2.1";
            src = lib.cleanSourceWith {
              src = ./ui;
              filter = path: type:
                let rel = lib.removePrefix (toString ./ui + "/") (toString path);
                in !(lib.hasPrefix "node_modules" rel
                  || lib.hasPrefix "dist" rel
                  || lib.hasPrefix "storybook-static" rel
                  || lib.hasPrefix "src-tauri" rel);
            };
            nativeBuildInputs = [ pkgs.bun pkgs.nodejs pkgs.cacert ];
            buildPhase = ''
              export HOME=$TMPDIR
              export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt
              bun install --frozen-lockfile --no-progress
              # The sandbox has no /usr/bin/env and bun symlinks .bin entries
              # (patchShebangs skips symlinks) — run the tools via node
              # directly instead of relying on their shebangs.
              node node_modules/typescript/bin/tsc --noEmit
              node node_modules/vite/bin/vite.js build
            '';
            installPhase = ''
              cp -r dist $out
            '';
            outputHashAlgo = "sha256";
            outputHashMode = "recursive";
            # NAR hash of the sandbox-built dist. It tracks every change
            # under ui/ since it was last set, not just the current commit —
            # so it goes stale as soon as any UI source moves, and a release
            # that only bumps versions can still need a new one. Re-run the
            # nix workflow and paste the "got:" hash from the mismatch error
            # (the local `nix hash path ui/dist` can differ slightly).
            outputHash = "sha256-ijlVCXYfJ+VeDmgWseZOQXGdeYXuLsoueXLbaCMZMKc=";
          };

          # Keep proto files (tonic codegen inputs) alongside the cargo sources.
          src = lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || (lib.hasSuffix ".proto" path);
          };

          commonArgs = {
            inherit src;
            pname = "otelview";
            version = "0.2.1";
            strictDeps = true;
            nativeBuildInputs = [ pkgs.protobuf ];
            PROTOC = "${pkgs.protobuf}/bin/protoc";
            DUCKDB_LIB_DIR = "${duckdb-static}";
            DUCKDB_INCLUDE_DIR = "${duckdb-static}";
            DUCKDB_STATIC = "1";
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          otelview = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            # rust-embed pulls the web UI from ui/dist at compile time.
            preBuild = ''
              mkdir -p ui
              rm -rf ui/dist
              cp -r ${webui} ui/dist
            '';
            doCheck = false;
            meta = {
              description = "Open-source, self-hosted OpenTelemetry viewer in a single binary";
              homepage = "https://github.com/tsirysndr/otelview";
              license = lib.licenses.mit;
              mainProgram = "otelview";
            };
          });
        in
        {
          inherit otelview webui duckdb-static;
          default = otelview;
        });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer
            protobuf
            bun
            unzip
            python3
            self.packages.${pkgs.stdenv.hostPlatform.system}.otelview
          ];
          PROTOC = "${pkgs.protobuf}/bin/protoc";
          DUCKDB_LIB_DIR = "${self.packages.${pkgs.stdenv.hostPlatform.system}.duckdb-static}";
          DUCKDB_INCLUDE_DIR = "${self.packages.${pkgs.stdenv.hostPlatform.system}.duckdb-static}";
          DUCKDB_STATIC = "1";
        };
      });
    };
}
