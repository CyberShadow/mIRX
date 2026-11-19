{
  description = "mirx bootstrap dev shell";

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
      # Wine with fixes for bugs our tests hit (see each patch's header).
      wine = pkgs.wineWow64Packages.stable.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [
          ./nix/patches/wine-usp10-script-cache-race.patch
        ];
      });
      # Fonts installed into every Wine test prefix (never shipped): a
      # monochrome emoji face so glyph fallback is exercised deterministically.
      wineFonts = pkgs.linkFarm "mirx-wine-fonts" [
        {
          name = "NotoEmoji.ttf";
          path = "${pkgs.noto-fonts-monochrome-emoji}/share/fonts/noto/NotoEmoji.ttf";
        }
      ];
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
          wine
          pkgs.xvfb-run
          mingwCc
        ];

        shellHook = ''
          export MIRX_ROOT="$PWD"
          export RUSTUP_HOME="$MIRX_ROOT/.rustup"
          export CARGO_HOME="$MIRX_ROOT/.cargo-home"
          export PATH="$CARGO_HOME/bin:$PATH"

          mkdir -p "$RUSTUP_HOME" "$CARGO_HOME"

          export PTHREADS="${mingwPthreads}"
          export MCFGTHREADS="${mingwMcfgthreads}"
          export CC_x86_64_pc_windows_gnu="x86_64-w64-mingw32-gcc"
          export CXX_x86_64_pc_windows_gnu="x86_64-w64-mingw32-g++"
          export CFLAGS_x86_64_pc_windows_gnu="-I$PTHREADS/include"
          export CXXFLAGS_x86_64_pc_windows_gnu="-I$PTHREADS/include"
          # No winedbg: a crash ends the process with Wine's one-line
          # report instead of stalling while winedbg --auto reads the debug
          # info of a 1 GB test executable.
          export WINEDLLOVERRIDES="mscoree,mshtml=;winedbg.exe=d"
          export MIRX_WINE_FONTS="${wineFonts}"

          ln -sfn "$PTHREADS" "$MIRX_ROOT/.nix-pthreads"
          ln -sfn "$MCFGTHREADS" "$MIRX_ROOT/.nix-mcfgthreads"

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
