#!/usr/bin/env rust-script
//! Install the Seele Rust toolchain from the local rust-seele checkout.
//!
//! Usage:
//!   ./install.rs [--target <triple>] [--toolchain <name>] [--std] [--skip-build]
//!
//! Defaults:
//!   target:     x86_64-seele
//!   toolchain:  seele
//!   build:      compiler/rustc + library/core
//!
//! Notes:
//! - Run this from the toolchain directory (where rust-seele/ exists).
//! - Requires rustup and a Rust host toolchain.

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

fn main() {
    let mut args = env::args().skip(1);
    let mut target = "x86_64-seele".to_string();
    let mut toolchain = "seele".to_string();
    let mut build_std = false;
    let mut skip_build = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => {
                target = args.next().unwrap_or_else(|| usage("--target requires a value"));
            }
            "--toolchain" => {
                toolchain = args.next().unwrap_or_else(|| usage("--toolchain requires a value"));
            }
            "--std" => build_std = true,
            "--skip-build" => skip_build = true,
            "--help" | "-h" => {
                usage("");
            }
            other => {
                usage(&format!("unknown argument: {other}"));
            }
        }
    }

    let cwd = env::current_dir().expect("failed to read current dir");
    let rust_dir = cwd.join("rust-seele");
    if !rust_dir.is_dir() {
        eprintln!(
            "error: rust-seele not found; run this from the toolchain dir (missing {})",
            rust_dir.display()
        );
        std::process::exit(1);
    }

    if !skip_build {
        let mut build_args = vec!["build", "--target", &target, "compiler/rustc", "library/core"];
        if build_std {
            build_args.push("library/std");
        }

        run_cmd(&rust_dir, "./x.py", &build_args)
            .unwrap_or_else(|err| die(&format!("x.py build failed: {err}")));
    }

    let host = rust_host_triple().unwrap_or_else(|err| die(&format!("failed to get host: {err}")));
    let stage2 = rust_dir.join("build").join(&host).join("stage2");
    if !stage2.is_dir() {
        die(&format!(
            "stage2 directory not found: {}",
            stage2.display()
        ));
    }

    run_cmd(&cwd, "rustup", &["toolchain", "link", &toolchain, stage2.to_str().unwrap()])
        .unwrap_or_else(|err| die(&format!("rustup toolchain link failed: {err}")));

    println!(
        "installed toolchain '{toolchain}' from {}",
        stage2.display()
    );
}

fn usage(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("error: {msg}");
    }
    eprintln!(
        "usage: ./install.rs [--target <triple>] [--toolchain <name>] [--std] [--skip-build]"
    );
    std::process::exit(2);
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn run_cmd<I, S>(dir: &Path, program: &str, args: I) -> Result<ExitStatus, String>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let status = Command::new(program)
        .current_dir(dir)
        .args(args)
        .status()
        .map_err(|err| format!("failed to run {program}: {err}"))?;
    if status.success() {
        Ok(status)
    } else {
        Err(format!("{program} exited with status {status}"))
    }
}

fn rust_host_triple() -> Result<String, String> {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|err| format!("failed to run rustc: {err}"))?;
    if !out.status.success() {
        return Err(format!("rustc -vV failed with status {}", out.status));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("host: ") {
            return Ok(rest.trim().to_string());
        }
    }
    Err("unable to parse rustc host triple".into())
}
