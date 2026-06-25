# mmatrix

`mmatrix` is a Windows-only native Win32 Matrix client in Rust. This first unit is the repository foundation: workspace layout, cross-toolchain wiring, minimal crates, and the script entry points that later units will extend into the real Matrix core and Win32 shell.

## Status

- Foundation only.
- `mmatrix-core` is an offline library stub with a small public API and unit coverage.
- `mmatrix-ui` is a minimal executable that supports `--self-test` and prints a deterministic success line.
- Live Matrix runs are intentionally not present yet; later units will gate them behind environment variables.

## Local Commands

Enter the dev shell from the repository root:

```sh
nix develop
```

The shell provisions local `.rustup` and `.cargo-home` caches, installs the stable Rust toolchain with `rustfmt`, enables the `x86_64-pc-windows-gnu` target, and exports the MinGW pthread include/lib settings validated by the spike.

Run the baseline checks:

```sh
nix develop -c cargo fmt --check
nix develop -c cargo test
```

Cross-build the Windows UI binary:

```sh
nix develop -c ./scripts/cross-build.sh
```

Run the Wine smoke test:

```sh
nix develop -c ./scripts/wine-smoke.sh
```

Run the DLL import gate:

```sh
nix develop -c ./scripts/dll-gate.sh
```

Measure the release artifact:

```sh
nix develop -c ./scripts/measure.sh
```
