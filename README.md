# Seele Toolchain

This repo holds the toolchain sources and scripts used to build Seele OS.

## Contents
- `rust/`: Rust toolchain source (submodule)
- `install.rs`: rust-script installer for the local Rust toolchain

## Prerequisites

You need a few tools installed before running the installer:

- `rustup` (to manage installed toolchains)
- A working host Rust toolchain (e.g. `nightly-x86_64-unknown-linux-gnu`) to build `rust`
- `rust-script` in `PATH` (used to run `install.rs`)

On Nix systems, run everything from the project `nix develop` shell so that LLVM and `libstdc++` are available.

## Usage
```bash
cd toolchain
./install.rs
```

Common flags:
- `--target <triple>` (default: `x86_64-seele`)
- `--toolchain <name>` (default: `seele`)
- `--no-std` (skip building std)
- `--skip-build` (only link with rustup)
- `--no-stage2` (link stage1 instead of stage2)
- `--force` (rebuild even if installed)
