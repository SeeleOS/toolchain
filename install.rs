#!/usr/bin/env rust-script
//! Install the Seele Rust toolchain from the local rust-seele checkout.
//!
//! Usage:
//!   ./install.rs [--target <triple>] [--toolchain <name>] [--no-std] [--skip-build] [--force] [--no-stage2]
//!
//! Defaults:
//!   target:     x86_64-seele
//!   toolchain:  seele
//!   build:      compiler/rustc + library/core + library/std (stage2)
//!
//! Notes:
//! - Run this from the toolchain directory (where rust-seele/ exists).
//! - Requires rustup and a Rust host toolchain.

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};

fn main() {
    let mut args = env::args().skip(1);
    let mut target = "x86_64-seele".to_string();
    let mut toolchain = "seele".to_string();
    let mut build_std = true;
    let mut skip_build = false;
    let mut force = false;
    let mut stage2 = true;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => {
                target = args.next().unwrap_or_else(|| usage("--target requires a value"));
            }
            "--toolchain" => {
                toolchain = args.next().unwrap_or_else(|| usage("--toolchain requires a value"));
            }
            "--std" => build_std = true,
            "--no-std" => build_std = false,
            "--skip-build" => skip_build = true,
            "--force" => force = true,
            "--stage2" => stage2 = true,
            "--no-stage2" => stage2 = false,
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

    if !force && toolchain_exists(&toolchain).unwrap_or(false) {
        println!("toolchain '{toolchain}' already installed; skipping (use --force to rebuild)");
        return;
    }

    if !skip_build {
        // 1) Build host compiler + host std (for build scripts / proc-macros).
        let mut host_args = vec!["build", "compiler/rustc"];
        if stage2 {
            host_args.insert(1, "--stage");
            host_args.insert(2, "2");
        }
        run_cmd(&rust_dir, "./x.py", &host_args)
            .unwrap_or_else(|err| die(&format!("x.py build (host) failed: {err}")));

        // Ensure the host standard library is available in the stage sysroot that we are
        // going to link via `rustup toolchain link`. This is needed so that build scripts
        // and proc-macros for host crates (e.g. when building relibc) can find `std` and
        // `core` for `x86_64-unknown-linux-gnu`.
        if build_std {
            let mut host_std_args = vec!["build", "--stage", if stage2 { "2" } else { "1" }, "library/std"];
            run_cmd(&rust_dir, "./x.py", &host_std_args)
                .unwrap_or_else(|err| die(&format!("x.py build (host std) failed: {err}")));
        }

        // 2) Build target std/core for the Seele target.
        let mut target_args = vec!["build", "--target", &target, "library/core"];
        if target == "x86_64-seele" {
            target_args.insert(1, "--warnings");
            target_args.insert(2, "warn");
        }
        if build_std {
            target_args.push("library/std");
        }
        if stage2 {
            target_args.insert(1, "--stage");
            target_args.insert(2, "2");
        }

        run_cmd(&rust_dir, "./x.py", &target_args)
            .unwrap_or_else(|err| die(&format!("x.py build (target {target}) failed: {err}")));
    }

    let host = rust_host_triple().unwrap_or_else(|err| die(&format!("failed to get host: {err}")));
    let stage_dir = rust_dir.join("build").join(&host).join(if stage2 { "stage2" } else { "stage1" });
    if !stage_dir.is_dir() {
        die(&format!(
            "stage directory not found: {} (try --stage2)",
            stage_dir.display()
        ));
    }

    // For the default Seele target, make the toolchain self-contained by
    // copying the relibc CRT objects and libc into the target's rustlib dir.
    if target == "x86_64-seele" {
        if let Err(err) = install_seele_runtime(&cwd, &stage_dir, &target) {
            die(&format!("failed to install Seele runtime: {err}"));
        }
    }

    run_cmd(&cwd, "rustup", &["toolchain", "link", &toolchain, stage_dir.to_str().unwrap()])
        .unwrap_or_else(|err| die(&format!("rustup toolchain link failed: {err}")));

    println!(
        "installed toolchain '{toolchain}' from {}",
        stage_dir.display()
    );
}

fn usage(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("error: {msg}");
    }
    eprintln!(
        "usage: ./install.rs [--target <triple>] [--toolchain <name>] [--no-std] [--skip-build] [--no-stage2]"
    );
    std::process::exit(2);
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn install_seele_runtime(workdir: &Path, stage_dir: &Path, target: &str) -> Result<(), String> {
    // `workdir` is the toolchain/ directory; the workspace root is its parent.
    let root = workdir
        .parent()
        .ok_or_else(|| "cannot determine workspace root (toolchain has no parent)".to_string())?;

    let relibc_dir = root
        .join("relibc-seele")
        .join("target")
        .join(target)
        .join("release");
    if !relibc_dir.is_dir() {
        return Err(format!(
            "relibc runtime dir not found: {} (build relibc-seele for {target} first)",
            relibc_dir.display()
        ));
    }

    let dst = stage_dir.join("lib").join("rustlib").join(target).join("lib");
    if !dst.is_dir() {
        return Err(format!(
            "target rustlib dir not found: {} (did x.py build library/std for {target}?)",
            dst.display()
        ));
    }

    fs::create_dir_all(&dst)
        .map_err(|e| format!("failed to create {}: {e}", dst.display()))?;

    // CRT entry/termination objects plus the C libraries that Rust expects
    // to link against by default (-lc, -lm, -lrt, -lpthread).
    let needed = ["crt0.o", "crti.o", "crtn.o", "libc.a", "libm.a", "librt.a", "libpthread.a"];

    for name in needed {
        let src = relibc_dir.join(name);
        if !src.is_file() {
            return Err(format!(
                "missing {} in {} (relibc-seele not fully built for {target})",
                name,
                relibc_dir.display()
            ));
        }
        let dst_file = dst.join(name);
        fs::copy(&src, &dst_file).map_err(|e| {
            format!("failed to copy {} -> {}: {e}", src.display(), dst_file.display())
        })?;
    }

    println!(
        "installed Seele runtime (CRT + libc) into {}",
        dst.display()
    );
    Ok(())
}

fn run_cmd<I, S>(dir: &Path, program: &str, args: I) -> Result<ExitStatus, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
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

fn toolchain_exists(name: &str) -> Result<bool, String> {
    let out = Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .map_err(|err| format!("failed to run rustup: {err}"))?;
    if !out.status.success() {
        return Err(format!("rustup toolchain list failed with status {}", out.status));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout.lines().any(|line| line.starts_with(name)))
}
