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

                rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
                    extensions = [ "rust-src" "rust-analyzer" ];
                };
                rustPlatform = pkgs.makeRustPlatform {
                    cargo = rustToolchain;
                    rustc = rustToolchain;
                };

                mingwPkgs = pkgs.pkgsCross.mingwW64;
                mingwCompiler = mingwPkgs.buildPackages.gcc;
                mingwRustflags = lib.strings.concatStringsSep " " [
                    "-L native=${mingwPkgs.windows.pthreads}/lib"
                    "-L native=${mingwPkgs.zstd}/bin"
                    "-L native=${mingwPkgs.udis86}/bin"
                    "-l dylib=zstd"
                    "-l dylib=udis86-0"
                ];

                libs = with pkgs; [
                    glib
                    gtk3
                    libGL
                    openssl
                    atk
                    libxkbcommon
                    wayland
                ];

                nativeBuildInputs = with pkgs; [
                    rustToolchain
                    pkg-config
                ];

                libraryPath = lib.makeLibraryPath libs;

                manifest = lib.importTOML ./Cargo.toml;
                packageName = manifest.package.name;
                packageVersion = manifest.workspace.package.version;

                package = rustPlatform.buildRustPackage {
                    nativeBuildInputs = nativeBuildInputs ++ [ pkgs.makeWrapper mingwCompiler ];
                    buildInputs = libs;

                    pname = packageName;
                    version = packageVersion;
                    src = lib.cleanSource ./.;

                    cargoLock = {
                        lockFile = ./Cargo.lock;
                        allowBuiltinFetchGit = true;
                    };

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
                    buildInputs = libs ++ [ mingwCompiler ];

                    LD_LIBRARY_PATH = libraryPath;
                    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = mingwRustflags;
                };
            });
}
