{
  description = "mmatrix bootstrap dev shell";

  inputs.nixpkgs.url = "nixpkgs";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        config.allowUnsupportedSystem = true;
      };
      mingwCc = pkgs.pkgsCross.mingwW64.stdenv.cc;
      mingwPthreads = pkgs.pkgsCross.mingwW64.windows.pthreads;
      mingwMcfgthreads = pkgs.pkgsCross.mingwW64.windows.mcfgthreads;
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = [
          pkgs.coreutils
          pkgs.file
          pkgs.findutils
          pkgs.gawk
          pkgs.gnugrep
          pkgs.gnused
          pkgs.imagemagick
          pkgs.pkg-config
          pkgs.rustup
          pkgs.util-linux
          pkgs.wineWow64Packages.stable
          pkgs.xvfb-run
          mingwCc
        ];

        shellHook = ''
          export MMATRIX_ROOT="$PWD"
          export RUSTUP_HOME="$MMATRIX_ROOT/.rustup"
          export CARGO_HOME="$MMATRIX_ROOT/.cargo-home"
          export PATH="$CARGO_HOME/bin:$PATH"

          mkdir -p "$RUSTUP_HOME" "$CARGO_HOME"

          export PTHREADS="${mingwPthreads}"
          export MCFGTHREADS="${mingwMcfgthreads}"
          export CC_x86_64_pc_windows_gnu="x86_64-w64-mingw32-gcc"
          export CXX_x86_64_pc_windows_gnu="x86_64-w64-mingw32-g++"
          export CFLAGS_x86_64_pc_windows_gnu="-I$PTHREADS/include"
          export CXXFLAGS_x86_64_pc_windows_gnu="-I$PTHREADS/include"
          export WINEDLLOVERRIDES="mscoree,mshtml="

          ln -sfn "$PTHREADS" "$MMATRIX_ROOT/.nix-pthreads"
          ln -sfn "$MCFGTHREADS" "$MMATRIX_ROOT/.nix-mcfgthreads"

          bootstrap_lock="$CARGO_HOME/.bootstrap.lock"
          (
            flock 9

            if ! rustup run stable cargo --version >/dev/null 2>&1; then
              rustup toolchain uninstall stable >/dev/null 2>&1 || true
              rustup toolchain install stable --profile minimal --component clippy --component rustfmt --target x86_64-pc-windows-gnu
            fi

            if ! rustup target list --installed --toolchain stable | grep -qx 'x86_64-pc-windows-gnu'; then
              rustup target add x86_64-pc-windows-gnu --toolchain stable
            fi

            if ! rustup component list --installed --toolchain stable | grep -q '^rustfmt'; then
              rustup component add rustfmt --toolchain stable
            fi

            if ! rustup component list --installed --toolchain stable | grep -q '^clippy'; then
              rustup component add clippy --toolchain stable
            fi
          ) 9>"$bootstrap_lock"
        '';
      };
    };
}
