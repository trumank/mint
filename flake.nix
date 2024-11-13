{
  description = "Mint development shell";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      system = "x86_64-linux";

      lib = nixpkgs.lib;
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs {
        inherit system overlays;
      };

      rustToolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
        extensions = [
          "rust-src"
          "rust-analyzer"
        ];
      };
      rustPlatform = pkgs.makeRustPlatform {
        cargo = rustToolchain;
        rustc = rustToolchain;
      };

      mingwPkgs = pkgs.pkgsCross.mingwW64;
      mingwCompiler = mingwPkgs.buildPackages.gcc;
      mingwRustflags = "-L ${mingwPkgs.windows.pthreads}/lib";
      mingwTool = name: "${mingwCompiler}/bin/${mingwCompiler.targetPrefix}${name}";

      libs = with pkgs; [
        gtk3
        libGL
        openssl
        atk
        libxkbcommon
        wayland
      ];

      buildTools = with pkgs; [
        rustToolchain
        pkg-config
        mingwCompiler
        makeWrapper
      ];

      libraryPath = lib.makeLibraryPath libs;

      manifest = lib.importTOML ./Cargo.toml;
      packageName = manifest.package.name;
      packageVersion = manifest.workspace.package.version;

      package = rustPlatform.buildRustPackage {
        nativeBuildInputs = buildTools;
        buildInputs = libs;

        pname = packageName;
        version = packageVersion;
        src = lib.cleanSource ./.;

        verbose = true;

        cargoLock = {
          lockFile = ./Cargo.lock;
          allowBuiltinFetchGit = true;
        };

        doCheck = false;

        preConfigure = ''
          export LD_LIBRARY_PATH="${libraryPath}"
          export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="${mingwRustflags}";
        '';

        postInstall = ''
          wrapProgram $out/bin/${packageName} \
            --prefix LD_LIBRARY_PATH : "${libraryPath}" \
            --prefix XDG_DATA_DIRS : "${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}"
        '';

        meta = with lib; {
          description = "Deep Rock Galactic mod loader and integration";
          license = licenses.mit;
          homepage = "https://github.com/trumank/mint";
          mainProgram = packageName;
        };
      };

      devShell = pkgs.mkShell {
        name = "mint";

        buildInputs = buildTools ++ libs;

        LD_LIBRARY_PATH = libraryPath;
        CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS = mingwRustflags;

        # Necessary for cross compiled build scripts, otherwise it will build as ELF format
        # https://docs.rs/cc/latest/cc/#external-configuration-via-environment-variables
        CC_x86_64_pc_windows_gnu = mingwTool "cc";
        AR_x86_64_pc_windows_gnu = mingwTool "ar";
      };
    in
    {
      packages.${system} = {
        ${packageName} = package;
        default = package;
      };

      devShells.${system}.default = devShell;
    };
}
