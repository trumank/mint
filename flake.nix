{
    description = "Mint development shell";

    inputs = {
        nixpkgs.url      = "github:nixos/nixpkgs/nixpkgs-unstable";
        flake-utils.url  = "github:numtide/flake-utils";
        rust-overlay.url = "github:oxalica/rust-overlay";
    };

    outputs = { nixpkgs, flake-utils, rust-overlay, ... }:
        flake-utils.lib.eachDefaultSystem (system:
            let
                lib = nixpkgs.lib;
                overlays = [ (import rust-overlay) ];
                pkgs = import nixpkgs {
                    inherit system overlays;
                };
                pkgsMinGW = pkgs.pkgsCross.mingwW64;
                toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

                nativeBuildInputs = with pkgs; [
                    toolchain
                    pkg-config
                ];

                buildInputs = with pkgs; [
                    pkgsMinGW.buildPackages.gcc
                    glib
                    gtk3
                    libGL
                    openssl
                    atk
                    libxkbcommon
                    wayland
                ];
            in {
                devShells.default = pkgs.mkShell {
                    name = "mint";

                    inherit nativeBuildInputs;
                    inherit buildInputs;

                    shellHook = let 
                        zstdLib = "${pkgsMinGW.zstd}/bin/libzstd.dll";
                        udisLib = "${pkgsMinGW.udis86}/bin/libudis86-0.dll";
                    in ''
                        test -f ./libzstd.dll || cp "${zstdLib}" "./"
                        test -f ./libudis86-0.dll || cp "${udisLib}" "./"
                    '';

                    LD_LIBRARY_PATH = lib.makeLibraryPath buildInputs;
                    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = lib.strings.concatStringsSep " " [
                        "-L native=${pkgsMinGW.windows.pthreads}/lib"
                        "-L native=${pkgsMinGW.zstd}/bin"
                        "-L native=${pkgsMinGW.udis86}/bin"
                        "-l dylib=zstd"
                        "-l dylib=udis86-0"
                    ];
                };
            });
}
