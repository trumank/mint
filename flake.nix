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

                hookDlls = {
                    "libzstd.dll" = pkgsMinGW.zstd;
                    "libudis86-0.dll" = pkgsMinGW.udis86;
                };
                pkgsMinGW = pkgs.pkgsCross.mingwW64;
                mingwRustflags = lib.strings.concatStringsSep " " [
                    "-L native=${pkgsMinGW.windows.pthreads}/lib"
                    "-L native=${pkgsMinGW.zstd}/bin"
                    "-L native=${pkgsMinGW.udis86}/bin"
                    "-l dylib=zstd"
                    "-l dylib=udis86-0"
                ];
                mingwCompiler = pkgsMinGW.buildPackages.gcc;

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

                manifest = (lib.importTOML ./Cargo.toml);
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
                    buildInputs = libs ++ [ mingwCompiler ];

                    shellHook = let 
                        tmpDir = "/tmp/drg-mint";
                        copy = lib.trivial.pipe hookDlls [
                            (lib.attrsets.mapAttrsToList (dll: pkg: ''
                                test -f "${tmpDir}/${dll}" || cp "${pkg}/bin/${dll}" "${tmpDir}"
                            ''))
                            lib.strings.concatLines
                        ];
                    in ''
                        mkdir -p "${tmpDir}"
                        ${copy}
                        echo "Copy files from ${tmpDir} into FSD/Binaries/Win64/ when using the Steam Flatpak"
                    '';

                    LD_LIBRARY_PATH = libraryPath;
                    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = mingwRustflags;
                };
            });
}
