{
  description = "K3 / BananaPi BPI-SM10 remote board workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    starrynix = {
      url = "github:StarryReverie/StarryNix-Derivations";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, starrynix, ... }:
    let
      systems = [
        "x86_64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f {
        inherit system;
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
      });
    in
    {
      devShells = forAllSystems ({ pkgs, system, ... }:
        let
          rust = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "riscv64gc-unknown-linux-musl" ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = [
              starrynix.packages.${system}.ostool
              rust
              pkgs.binutils
              pkgs.curl
              pkgs.file
              pkgs.jq
              pkgs.libsecret
              pkgs.python3
              pkgs.util-linux
              pkgs.gnugrep
              pkgs.coreutils
            ];

            shellHook = ''
              export PATH="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin:$PATH"
              mkdir -p logs
              echo
              echo "K3 workspace ready ($(ostool --version 2>/dev/null || echo 'ostool: not found'))"
              echo
              echo "  ostool login                 # browser device authorization"
              echo "  ostool auth status           # expect: credential: OAuth"
              echo "  ostool board ls              # check BananaPi lease"
              echo "  ostool board connect -b BananaPi"
              echo "  ./scripts/capture-session.sh # same, but keeps a log in ./logs"
              echo "  ./scripts/smoke-test.py      # automated U-Boot smoke test"
              echo
              echo "  rust cross build:"
              echo "    cargo build --manifest-path rust-hello/Cargo.toml --release --target riscv64gc-unknown-linux-musl"
              echo
              echo "  interrupt autoboot: Ctrl+C during 'Autoboot in 3 seconds'"
              echo "  serial exit: Ctrl+A, release, then x"
              echo
            '';
          };
        });
    };
}
