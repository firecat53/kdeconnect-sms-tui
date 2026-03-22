{
  description = "TUI SMS client via KDE Connect";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
        ];

        buildInputs = with pkgs; [
          dbus
          libheif
        ];

      in {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "kdeconnect-sms-tui";
          version = "0.1.0";
          src = ./.;
          useFetchCargoVendor = true;
          cargoHash = "";

          inherit nativeBuildInputs buildInputs;

          meta = with pkgs.lib; {
            description = "TUI SMS client via KDE Connect";
            license = licenses.mit;
            mainProgram = "kdeconnect-sms-tui";
          };
        };

        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
            cargo-watch
            cargo-nextest
          ]);

          RUST_BACKTRACE = "1";
          RUST_LOG = "debug";
        };
      }
    );
}
