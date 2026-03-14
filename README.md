# Seele Toolchain

This repo holds the toolchain sources and scripts used to build Seele OS.

## Contents
- `rust-seele/`: Rust toolchain source (submodule)
- `install.rs`: rust-script installer for the local Rust toolchain

## Usage
```bash
cd toolchain
./install.rs
```

Common flags:
- `--target <triple>` (default: `x86_64-seele`)
- `--toolchain <name>` (default: `seele`)
- `--std` (also build std)
- `--skip-build` (only link with rustup)
- `--force` (rebuild even if installed)
