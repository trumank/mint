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

                toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
                    extensions = [ "rust-src" "rust-analyzer" ];
                };

                rustPlatform = pkgs.makeRustPlatform {
                    cargo = toolchain;
                    rustc = toolchain;
                };

                pkgsMinGW = pkgs.pkgsCross.mingwW64;
                mingwRustflags = lib.strings.concatStringsSep " " [
                    "-L native=${pkgsMinGW.windows.pthreads}/lib"
                    "-L native=${pkgsMinGW.zstd}/bin"
                    "-L native=${pkgsMinGW.udis86}/bin"
                    "-l dylib=zstd"
                    "-l dylib=udis86-0"
                ];

                nativeBuildInputs = with pkgs; [
                    toolchain
                    pkgsMinGW.buildPackages.gcc
                    pkg-config
                    makeWrapper
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

                libraryPath = lib.makeLibraryPath buildInputs;

                manifest = (lib.importTOML ./Cargo.toml);
                packageName = manifest.package.name;
                packageVersion = manifest.workspace.package.version;

                package = rustPlatform.buildRustPackage {
                    inherit nativeBuildInputs;
                    inherit buildInputs;

                    pname = packageName;
                    version = packageVersion;
                    src = lib.cleanSource ./.;

                    cargoLock = {
                        lockFile = ./Cargo.lock;
                        allowBuiltinFetchGit = true;
                    };

                    # checkType = "debug";
                    doCheck = false;

                    preConfigure = ''
                        export LD_LIBRARY_PATH="${libraryPath}"
                        export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="${mingwRustflags}"
                    '';

                    postFixup = ''
                        wrapProgram $out/bin/${packageName} --suffix LD_LIBRARY_PATH : ${libraryPath}
                    '';
                };
            in {
                packages = {
                    ${packageName} = package;
                    default = package;
                };

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

                    LD_LIBRARY_PATH = libraryPath;
                    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = mingwRustflags;
                };
            });
}
