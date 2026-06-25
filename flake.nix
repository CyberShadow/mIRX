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
          export CFLAGS_x86_64_pc_windows_gnu="-I$PTHREADS/include"
          export CXXFLAGS_x86_64_pc_windows_gnu="-I$PTHREADS/include"
          export WINEDLLOVERRIDES="mscoree,mshtml="

          ln -sfn "$PTHREADS" "$MMATRIX_ROOT/.nix-pthreads"
          ln -sfn "$MCFGTHREADS" "$MMATRIX_ROOT/.nix-mcfgthreads"

          if ! rustup toolchain list | grep -q '^stable'; then
            rustup toolchain install stable --profile minimal --component rustfmt --target x86_64-pc-windows-gnu
          fi

          if ! rustup target list --installed --toolchain stable | grep -qx 'x86_64-pc-windows-gnu'; then
            rustup target add x86_64-pc-windows-gnu --toolchain stable
          fi

          if ! rustup component list --installed --toolchain stable | grep -q '^rustfmt'; then
            rustup component add rustfmt --toolchain stable
          fi
        '';
      };
    };
}
