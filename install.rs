#!/usr/bin/env rust-script
//! Install the Seele Rust toolchain from the local rust checkout.
//!
//! Usage:
//!   ./install.rs [--target <triple>] [--toolchain <name>] [--no-std] [--skip-build] [--no-force] [--no-stage2] [--no-llvm-cxx]
//!
//! Defaults:
//!   target:     x86_64-seele
//!   toolchain:  seele
//!   build:      compiler/rustc + library/core + library/std (stage2)
//!   force:      enabled
//!
//! Notes:
//! - Run this from the toolchain directory (where rust/ exists).
//! - Requires rustup and a Rust host toolchain.

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};

fn main() {
    ensure_sysroot_mounted();
    let config = Config::parse();
    install_llvm(&config);
    install_rust(&config);
}

struct Config {
    target: String,
    toolchain: String,
    build_std: bool,
    skip_build: bool,
    force: bool,
    stage2: bool,
    llvm_cxx: bool,
}

impl Config {
    fn parse() -> Self {
        let mut args = env::args().skip(1);
        let mut config = Self {
            target: "x86_64-seele".to_string(),
            toolchain: "seele".to_string(),
            build_std: true,
            skip_build: false,
            force: true,
            stage2: true,
            llvm_cxx: true,
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--target" => {
                    config.target = args
                        .next()
                        .unwrap_or_else(|| usage("--target requires a value"));
                }
                "--toolchain" => {
                    config.toolchain = args
                        .next()
                        .unwrap_or_else(|| usage("--toolchain requires a value"));
                }
                "--std" => config.build_std = true,
                "--no-std" => config.build_std = false,
                "--skip-build" => config.skip_build = true,
                "--force" => config.force = true,
                "--no-force" => config.force = false,
                "--stage2" => config.stage2 = true,
                "--no-stage2" => config.stage2 = false,
                "--llvm-cxx" => config.llvm_cxx = true,
                "--no-llvm-cxx" => config.llvm_cxx = false,
                "--help" | "-h" => usage(""),
                other => usage(&format!("unknown argument: {other}")),
            }
        }

        config
    }

    fn llvm_target(&self) -> String {
        match self.target.as_str() {
            "x86_64-seele" => "x86_64-unknown-seele".to_string(),
            other => other.to_string(),
        }
    }
}

fn install_llvm(config: &Config) {
    let cwd = env::current_dir().expect("failed to read current dir");
    let llvm_dir = cwd.join("llvm-project");
    if !llvm_dir.is_dir() {
        die(&format!(
            "llvm-project not found; run this from the toolchain dir (missing {})",
            llvm_dir.display()
        ));
    }

    let root = cwd
        .parent()
        .unwrap_or_else(|| die("cannot determine workspace root (toolchain has no parent)"));
    let prefix = root.join(".llvm");
    let sysroot = root.join("sysroot");
    let build_dir = llvm_dir.join("build-seele");
    let llvm_target = config.llvm_target();

    fs::create_dir_all(&prefix)
        .unwrap_or_else(|err| die(&format!("failed to create {}: {err}", prefix.display())));

    let llvm_runtimes = if config.llvm_cxx {
        vec![
            "libunwind".to_string(),
            "libcxxabi".to_string(),
            "libcxx".to_string(),
        ]
    } else {
        vec!["compiler-rt".to_string()]
    };

    let mut cmake_args: Vec<String> = vec![
        "-S".into(),
        "llvm".into(),
        "-B".into(),
        build_dir
            .file_name()
            .unwrap_or_else(|| die("invalid LLVM build directory"))
            .to_string_lossy()
            .into_owned(),
        "-G".into(),
        "Ninja".into(),
        format!("-UBUILTINS_{llvm_target}_CMAKE_C_FLAGS"),
        format!("-UBUILTINS_{llvm_target}_CMAKE_ASM_FLAGS"),
        "-DCMAKE_BUILD_TYPE=Release".into(),
        "-DLLVM_ENABLE_PROJECTS=clang;lld".into(),
        format!("-DLLVM_ENABLE_RUNTIMES={}", llvm_runtimes.join(";")),
        "-DLLVM_TARGETS_TO_BUILD=X86".into(),
        format!("-DLLVM_BUILTIN_TARGETS={llvm_target}"),
        format!("-DBUILTINS_{llvm_target}_CMAKE_SYSTEM_NAME=Seele"),
        format!(
            "-DBUILTINS_{llvm_target}_CMAKE_SYSROOT={}",
            sysroot.display()
        ),
        format!("-DBUILTINS_{llvm_target}_CMAKE_C_COMPILER_TARGET={llvm_target}"),
        format!("-DBUILTINS_{llvm_target}_CMAKE_ASM_COMPILER_TARGET={llvm_target}"),
        format!(
            "-DBUILTINS_{llvm_target}_CMAKE_C_FLAGS=--sysroot={}",
            sysroot.display()
        ),
        format!(
            "-DBUILTINS_{llvm_target}_CMAKE_ASM_FLAGS=--sysroot={}",
            sysroot.display()
        ),
        format!("-DBUILTINS_{llvm_target}_COMPILER_RT_BUILD_CRT=OFF"),
        format!("-DCMAKE_INSTALL_PREFIX={}", prefix.display()),
    ];

    if config.llvm_cxx {
        cmake_args.extend([
            format!("-DLLVM_RUNTIME_TARGETS={llvm_target}"),
            format!("-URUNTIMES_{llvm_target}_LIBCXX_ENABLE_LOCALIZATION"),
            format!("-URUNTIMES_{llvm_target}_LIBCXX_ENABLE_FILESYSTEM"),
            // CMake doesn't know about Seele as a platform yet; treat the
            // runtime sub-build as a generic cross target and let clang's
            // target triple drive the actual code generation.
            format!("-DRUNTIMES_{llvm_target}_CMAKE_SYSTEM_NAME=Generic"),
            format!(
                "-DRUNTIMES_{llvm_target}_CMAKE_SYSROOT={}",
                sysroot.display()
            ),
            format!("-DRUNTIMES_{llvm_target}_CMAKE_C_COMPILER_TARGET={llvm_target}"),
            format!("-DRUNTIMES_{llvm_target}_CMAKE_CXX_COMPILER_TARGET={llvm_target}"),
            format!("-DRUNTIMES_{llvm_target}_CMAKE_ASM_COMPILER_TARGET={llvm_target}"),
            format!("-DRUNTIMES_{llvm_target}_CMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY"),
            format!(
                "-DRUNTIMES_{llvm_target}_CMAKE_C_FLAGS=--sysroot={} -D_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE",
                sysroot.display()
            ),
            format!(
                "-DRUNTIMES_{llvm_target}_CMAKE_CXX_FLAGS=--sysroot={} -D_LIBCPP_PROVIDES_DEFAULT_RUNE_TABLE",
                sysroot.display()
            ),
            format!(
                "-DRUNTIMES_{llvm_target}_CMAKE_ASM_FLAGS=--sysroot={}",
                sysroot.display()
            ),
            format!("-DRUNTIMES_{llvm_target}_LIBUNWIND_USE_COMPILER_RT=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBUNWIND_IS_BAREMETAL=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXXABI_USE_COMPILER_RT=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXXABI_BAREMETAL=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXXABI_ENABLE_THREADS=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXXABI_HAS_PTHREAD_API=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_USE_COMPILER_RT=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXXABI_USE_LLVM_UNWINDER=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_CXX_ABI=libcxxabi"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_ENABLE_LOCALIZATION=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_ENABLE_FILESYSTEM=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_ENABLE_THREADS=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_HAS_PTHREAD_API=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_LINK_FLAGS=-Wl,-soname,libc++.so.1"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXXABI_LINK_FLAGS=-Wl,-soname,libc++abi.so.1"),
            format!("-DRUNTIMES_{llvm_target}_LIBUNWIND_LINK_FLAGS=-Wl,-soname,libunwind.so.1"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXX_ENABLE_SHARED=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBCXXABI_ENABLE_SHARED=ON"),
            format!("-DRUNTIMES_{llvm_target}_LIBUNWIND_ENABLE_SHARED=ON"),
            format!("-DRUNTIMES_{llvm_target}_LLVM_USES_LIBSTDCXX=OFF"),
            format!("-DRUNTIMES_{llvm_target}_LLVM_DEFAULT_TO_GLIBCXX_USE_CXX11_ABI=OFF"),
        ]);
    }

    run_cmd_owned(&llvm_dir, "cmake", cmake_args.drain(..))
        .unwrap_or_else(|err| die(&format!("failed to configure LLVM: {err}")));

    let jobs = std::thread::available_parallelism()
        .map(|n| n.get().to_string())
        .unwrap_or_else(|_| "1".to_string());

    run_cmd_owned(
        &llvm_dir,
        "ninja",
        vec![
            "-C".into(),
            build_dir
                .file_name()
                .unwrap_or_else(|| die("invalid LLVM build directory"))
                .to_string_lossy()
                .into_owned(),
            "-j".into(),
            jobs,
            "install".into(),
        ],
    )
    .unwrap_or_else(|err| die(&format!("failed to build/install LLVM: {err}")));

    install_llvm_sysroot_link(&prefix, &sysroot)
        .unwrap_or_else(|err| die(&format!("failed to link LLVM sysroot: {err}")));

    if config.llvm_cxx {
        install_libcpp(&prefix, &sysroot, &llvm_target)
            .unwrap_or_else(|err| die(&format!("failed to install libc++ into sysroot: {err}")));
    }

    println!("installed LLVM toolchain into {}", prefix.display());
}

fn install_rust(config: &Config) {
    let cwd = env::current_dir().expect("failed to read current dir");
    let rust_dir = cwd.join("rust");
    if !rust_dir.is_dir() {
        eprintln!(
            "error: rust not found; run this from the toolchain dir (missing {})",
            rust_dir.display()
        );
        std::process::exit(1);
    }

    let toolchain_already_exists =
        !config.force && toolchain_exists(&config.toolchain).unwrap_or(false);
    if toolchain_already_exists {
        println!(
            "toolchain '{}' already installed; skipping rebuild and refreshing linked tools",
            config.toolchain
        );
    }

    if !config.skip_build && !toolchain_already_exists {
        // 1) Build host compiler + host std (for build scripts / proc-macros).
        let mut host_args = vec!["build", "--warnings", "warn", "compiler/rustc"];
        if config.stage2 {
            host_args.insert(3, "--stage");
            host_args.insert(4, "2");
        }
        run_cmd(&rust_dir, "./x.py", &host_args)
            .unwrap_or_else(|err| die(&format!("x.py build (host) failed: {err}")));

        // Ensure the host standard library is available in the stage sysroot that we are
        // going to link via `rustup toolchain link`. This is needed so that build scripts
        // and proc-macros for host crates (e.g. when building relibc) can find `std` and
        // `core` for `x86_64-unknown-linux-gnu`.
        if config.build_std {
            let host_std_args = vec![
                "build",
                "--warnings",
                "warn",
                "--stage",
                if config.stage2 { "2" } else { "1" },
                "library/std",
            ];
            run_cmd(&rust_dir, "./x.py", &host_std_args)
                .unwrap_or_else(|err| die(&format!("x.py build (host std) failed: {err}")));
        }

        // 2) Build target std/core for the Seele target.
        // Let bootstrap drive the dependency graph itself. Building `library/std`
        // already pulls in `core`, `alloc`, and `compiler_builtins`; passing both
        // `library/core` and `library/std` can confuse custom-target builds.
        let mut target_args = vec!["build", "--target", &config.target];
        if config.target == "x86_64-seele" {
            target_args.insert(1, "--warnings");
            target_args.insert(2, "warn");
        }
        if config.build_std {
            target_args.push("library/std");
        } else {
            target_args.push("library/core");
        }
        if config.stage2 {
            target_args.insert(1, "--stage");
            target_args.insert(2, "2");
        }

        run_cmd(&rust_dir, "./x.py", &target_args).unwrap_or_else(|err| {
            die(&format!(
                "x.py build (target {}) failed: {err}",
                config.target
            ))
        });
    }

    let host = rust_host_triple().unwrap_or_else(|err| die(&format!("failed to get host: {err}")));
    let stage_dir =
        rust_dir
            .join("build")
            .join(&host)
            .join(if config.stage2 { "stage2" } else { "stage1" });
    if !stage_dir.is_dir() {
        die(&format!(
            "stage directory not found: {} (try --stage2)",
            stage_dir.display()
        ));
    }

    sync_rustlib(&rust_dir, &host, &stage_dir, &host)
        .unwrap_or_else(|err| die(&format!("failed to install host rustlib: {err}")));
    sync_rustlib(&rust_dir, &host, &stage_dir, &config.target)
        .unwrap_or_else(|err| die(&format!("failed to install target rustlib: {err}")));

    // For the default Seele target, make the toolchain self-contained by
    // copying the relibc CRT objects and libc into the target's rustlib dir.
    if config.target == "x86_64-seele" {
        if let Err(err) = install_seele_runtime(&cwd, &stage_dir, &config.target) {
            die(&format!("failed to install Seele runtime: {err}"));
        }
    }

    let llvm_prefix = cwd
        .parent()
        .unwrap_or_else(|| die("cannot determine workspace root (toolchain has no parent)"))
        .join(".llvm");
    if let Err(err) = install_llvm_bin_tools(&llvm_prefix, &stage_dir, &["llvm-ar", "llvm-ranlib"])
    {
        die(&format!(
            "failed to install LLVM bin tools into Rust toolchain: {err}"
        ));
    }

    run_cmd(
        &cwd,
        "rustup",
        &[
            "toolchain",
            "link",
            &config.toolchain,
            stage_dir.to_str().unwrap(),
        ],
    )
    .unwrap_or_else(|err| die(&format!("rustup toolchain link failed: {err}")));

    println!(
        "installed toolchain '{}' from {}",
        config.toolchain,
        stage_dir.display()
    );
}

fn usage(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("error: {msg}");
    }
    eprintln!(
        "usage: ./install.rs [--target <triple>] [--toolchain <name>] [--no-std] [--skip-build] [--no-stage2] [--no-llvm-cxx]"
    );
    std::process::exit(2);
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn install_llvm_sysroot_link(prefix: &Path, sysroot: &Path) -> Result<(), String> {
    let link_path = prefix.join("sysroot");

    if link_path.exists() || link_path.symlink_metadata().is_ok() {
        let meta = link_path
            .symlink_metadata()
            .map_err(|e| format!("failed to inspect {}: {e}", link_path.display()))?;
        if meta.file_type().is_symlink() {
            fs::remove_file(&link_path)
                .map_err(|e| format!("failed to remove {}: {e}", link_path.display()))?;
        } else {
            return Err(format!(
                "{} exists and is not a symlink",
                link_path.display()
            ));
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(sysroot, &link_path).map_err(|e| {
            format!(
                "failed to create symlink {} -> {}: {e}",
                link_path.display(),
                sysroot.display()
            )
        })?;
    }

    #[cfg(not(unix))]
    {
        return Err("creating the LLVM sysroot symlink is only implemented on unix".to_string());
    }

    Ok(())
}

fn install_libcpp(prefix: &Path, sysroot: &Path, llvm_target: &str) -> Result<(), String> {
    let src_include = prefix.join("include").join("c++").join("v1");
    if !src_include.is_dir() {
        return Err(format!(
            "libc++ headers not found at {}",
            src_include.display()
        ));
    }

    let dst_include = sysroot.join("libs").join("include").join("c++").join("v1");
    remove_installed_path(&dst_include)?;
    run_cmd(
        Path::new("/"),
        "sudo",
        [
            "mkdir",
            "-p",
            dst_include
                .parent()
                .ok_or_else(|| format!("invalid libc++ include path {}", dst_include.display()))?
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", dst_include.display()))?,
        ],
    )?;
    run_cmd(
        Path::new("/"),
        "sudo",
        [
            "cp",
            "-a",
            src_include
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", src_include.display()))?,
            dst_include
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", dst_include.display()))?,
        ],
    )?;

    let src_target_include = prefix
        .join("include")
        .join(llvm_target)
        .join("c++")
        .join("v1");
    if !src_target_include.is_dir() {
        return Err(format!(
            "target-specific libc++ headers not found at {}",
            src_target_include.display()
        ));
    }
    let dst_target_include = sysroot
        .join("libs")
        .join("include")
        .join(llvm_target)
        .join("c++")
        .join("v1");
    remove_installed_path(&dst_target_include)?;
    run_cmd(
        Path::new("/"),
        "sudo",
        [
            "mkdir",
            "-p",
            dst_target_include
                .parent()
                .ok_or_else(|| {
                    format!(
                        "invalid target libc++ include path {}",
                        dst_target_include.display()
                    )
                })?
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", dst_target_include.display()))?,
        ],
    )?;
    run_cmd(
        Path::new("/"),
        "sudo",
        [
            "cp",
            "-a",
            src_target_include
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", src_target_include.display()))?,
            dst_target_include
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", dst_target_include.display()))?,
        ],
    )?;

    let src_lib_dir = prefix.join("lib").join(llvm_target);
    let dst_lib_dir = sysroot.join("libs").join("lib_binaries");
    run_cmd(
        Path::new("/"),
        "sudo",
        [
            "mkdir",
            "-p",
            dst_lib_dir
                .to_str()
                .ok_or_else(|| format!("non-utf8 path {}", dst_lib_dir.display()))?,
        ],
    )?;

    for name in [
        "libc++.a",
        "libc++abi.a",
        "libunwind.a",
        "libc++.so",
        "libc++abi.so",
        "libunwind.so",
    ] {
        let src = src_lib_dir.join(name);
        if !src.is_file() {
            return Err(format!("missing {} at {}", name, src.display()));
        }
        let dst = dst_lib_dir.join(name);
        run_cmd(
            Path::new("/"),
            "sudo",
            [
                "cp",
                "-f",
                src.to_str()
                    .ok_or_else(|| format!("non-utf8 path {}", src.display()))?,
                dst.to_str()
                    .ok_or_else(|| format!("non-utf8 path {}", dst.display()))?,
            ],
        )?;
    }

    for (link, target) in [
        ("libc++.so.1", "libc++.so"),
        ("libc++abi.so.1", "libc++abi.so"),
        ("libunwind.so.1", "libunwind.so"),
    ] {
        let prefix_link = src_lib_dir.join(link);
        #[cfg(unix)]
        {
            let _ = fs::remove_file(&prefix_link);
            std::os::unix::fs::symlink(target, &prefix_link).map_err(|e| {
                format!(
                    "failed to create symlink {} -> {}: {e}",
                    prefix_link.display(),
                    target
                )
            })?;
        }

        let dst = dst_lib_dir.join(link);
        run_cmd(
            Path::new("/"),
            "sudo",
            [
                "ln",
                "-sfn",
                target,
                dst.to_str()
                    .ok_or_else(|| format!("non-utf8 path {}", dst.display()))?,
            ],
        )?;
    }

    println!(
        "installed libc++ headers and libraries into {}",
        sysroot.display()
    );
    Ok(())
}

fn remove_installed_path(path: &Path) -> Result<(), String> {
    if !path.exists() && path.symlink_metadata().is_err() {
        return Ok(());
    }

    run_cmd(
        Path::new("/"),
        "sudo",
        [
            "rm",
            "-rf",
            path.to_str()
                .ok_or_else(|| format!("non-utf8 path {}", path.display()))?,
        ],
    )?;

    Ok(())
}

fn install_seele_runtime(workdir: &Path, stage_dir: &Path, target: &str) -> Result<(), String> {
    // `workdir` is the toolchain/ directory; the workspace root is its parent.
    let root = workdir
        .parent()
        .ok_or_else(|| "cannot determine workspace root (toolchain has no parent)".to_string())?;

    let relibc_dir = root
        .join("relibc")
        .join("target")
        .join(target)
        .join("release");
    if !relibc_dir.is_dir() {
        return Err(format!(
            "relibc runtime dir not found: {} (build relibc for {target} first)",
            relibc_dir.display()
        ));
    }

    let dst = stage_dir
        .join("lib")
        .join("rustlib")
        .join(target)
        .join("lib");
    if !dst.is_dir() {
        return Err(format!(
            "target rustlib dir not found: {} (did x.py build library/std for {target}?)",
            dst.display()
        ));
    }

    fs::create_dir_all(&dst).map_err(|e| format!("failed to create {}: {e}", dst.display()))?;

    // CRT entry/termination objects plus the C libraries that Rust expects
    // to link against by default (-lc, -lm, -lrt, -lpthread).
    let needed = [
        "crt0.o",
        "crti.o",
        "crtn.o",
        "libm.a",
        "librt.a",
        "libc.so",
        "libpthread.a",
    ];

    for name in needed {
        let src = relibc_dir.join(name);
        if !src.is_file() {
            return Err(format!(
                "missing {} in {} (relibc not fully built for {target})",
                name,
                relibc_dir.display()
            ));
        }
        let dst_file = dst.join(name);
        fs::copy(&src, &dst_file).map_err(|e| {
            format!(
                "failed to copy {} -> {}: {e}",
                src.display(),
                dst_file.display()
            )
        })?;
    }

    install_symlink("libc.so", &dst.join("libc.so.6"))?;

    println!(
        "installed Seele runtime (CRT + static/shared libc) into {}",
        dst.display()
    );
    Ok(())
}

fn install_symlink(target: &str, link_path: &Path) -> Result<(), String> {
    if link_path.exists() || link_path.symlink_metadata().is_ok() {
        fs::remove_file(link_path)
            .map_err(|e| format!("failed to remove {}: {e}", link_path.display()))?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link_path).map_err(|e| {
            format!(
                "failed to create symlink {} -> {}: {e}",
                link_path.display(),
                target
            )
        })?;
    }

    #[cfg(not(unix))]
    {
        return Err(format!(
            "creating symlink {} -> {} is only implemented on unix",
            link_path.display(),
            target
        ));
    }

    Ok(())
}

fn sync_rustlib(
    rust_dir: &Path,
    build_host: &str,
    stage_dir: &Path,
    triple: &str,
) -> Result<(), String> {
    let build_dir = rust_dir.join("build").join(build_host);
    let candidates = [
        build_dir
            .join("stage1-std")
            .join(triple)
            .join("dist")
            .join("deps"),
        build_dir.join("stage1").join("lib").join("rustlib").join(triple).join("lib"),
        build_dir.join("stage2").join("lib").join("rustlib").join(triple).join("lib"),
    ];
    let src = candidates
        .iter()
        .find(|path| path.is_dir())
        .ok_or_else(|| {
            let tried = candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("no rustlib source dir found for {triple}; tried: {tried}")
        })?;

    let dst = stage_dir.join("lib").join("rustlib").join(triple).join("lib");
    fs::create_dir_all(&dst).map_err(|e| format!("failed to create {}: {e}", dst.display()))?;

    ensure_rustlib_is_nonempty(src, triple)?;

    for entry in fs::read_dir(src)
        .map_err(|e| format!("failed to read rustlib dir {}: {e}", src.display()))?
    {
        let entry =
            entry.map_err(|e| format!("failed to read entry in {}: {e}", src.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let dst_file = dst.join(entry.file_name());
        fs::copy(&path, &dst_file).map_err(|e| {
            format!(
                "failed to copy {} -> {}: {e}",
                path.display(),
                dst_file.display()
            )
        })?;
    }

    println!("installed rustlib for {triple} into {}", dst.display());
    Ok(())
}

fn ensure_rustlib_is_nonempty(src: &Path, triple: &str) -> Result<(), String> {
    let required_prefixes = if triple.contains("seele") {
        ["libcore-", "libcompiler_builtins-", "liballoc-", "libstd-"].as_slice()
    } else {
        ["libcore-", "liballoc-", "libstd-"].as_slice()
    };

    for prefix in required_prefixes {
        let mut matched = false;
        for entry in fs::read_dir(src)
            .map_err(|e| format!("failed to read rustlib dir {}: {e}", src.display()))?
        {
            let entry =
                entry.map_err(|e| format!("failed to read entry in {}: {e}", src.display()))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with(prefix) || !name.ends_with(".rlib") {
                continue;
            }

            matched = true;
            let len = fs::metadata(&path)
                .map_err(|e| format!("failed to stat {}: {e}", path.display()))?
                .len();
            if len == 0 {
                return Err(format!(
                    "rustlib source {} is empty; rebuild the {triple} standard library first",
                    path.display()
                ));
            }
        }

        if !matched {
            return Err(format!(
                "rustlib source {} is missing {}*.rlib for {triple}",
                src.display(),
                prefix
            ));
        }
    }

    Ok(())
}

fn install_llvm_bin_tools(prefix: &Path, stage_dir: &Path, tools: &[&str]) -> Result<(), String> {
    let llvm_bin = prefix.join("bin");
    if !llvm_bin.is_dir() {
        return Err(format!("LLVM bin dir not found: {}", llvm_bin.display()));
    }

    let stage_bin = stage_dir.join("bin");
    if !stage_bin.is_dir() {
        return Err(format!(
            "Rust toolchain bin dir not found: {}",
            stage_bin.display()
        ));
    }

    for tool in tools {
        let src = llvm_bin.join(tool);
        if !src.is_file() {
            return Err(format!("missing LLVM tool {} at {}", tool, src.display()));
        }

        let dst = stage_bin.join(tool);
        if dst.exists() || dst.symlink_metadata().is_ok() {
            fs::remove_file(&dst)
                .map_err(|e| format!("failed to remove {}: {e}", dst.display()))?;
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&src, &dst).map_err(|e| {
                format!(
                    "failed to create symlink {} -> {}: {e}",
                    dst.display(),
                    src.display()
                )
            })?;
        }

        #[cfg(not(unix))]
        {
            fs::copy(&src, &dst).map_err(|e| {
                format!("failed to copy {} -> {}: {e}", src.display(), dst.display())
            })?;
        }
    }

    println!("installed LLVM bin tools into {}", stage_bin.display());
    Ok(())
}

fn ensure_sysroot_mounted() {
    let cwd = env::current_dir().unwrap_or_else(|err| die(&format!("failed to read current dir: {err}")));
    let root = cwd
        .parent()
        .unwrap_or_else(|| die("cannot determine workspace root (toolchain has no parent)"));
    let sysroot = root.join("sysroot");
    let disk_img = root.join("disk.img");

    fs::create_dir_all(&sysroot)
        .unwrap_or_else(|err| die(&format!("failed to create {}: {err}", sysroot.display())));

    let mount_status = Command::new("mountpoint")
        .arg("-q")
        .arg(&sysroot)
        .status()
        .unwrap_or_else(|err| die(&format!("failed to run mountpoint: {err}")));

    if mount_status.success() {
        return;
    }

    if !matches!(mount_status.code(), Some(1 | 32)) {
        die(&format!(
            "mountpoint -q {} exited with status {}",
            sysroot.display(),
            mount_status
        ));
    }

    if !disk_img.is_file() {
        die(&format!(
            "disk image not found: {} (create/mount it before installing the toolchain)",
            disk_img.display()
        ));
    }

    run_cmd(
        root,
        "sudo",
        [
            "mount",
            "-o",
            "loop",
            disk_img
                .to_str()
                .unwrap_or_else(|| die(&format!("non-utf8 path {}", disk_img.display()))),
            sysroot
                .to_str()
                .unwrap_or_else(|| die(&format!("non-utf8 path {}", sysroot.display()))),
        ],
    )
    .unwrap_or_else(|err| die(&format!("failed to mount sysroot: {err}")));
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

fn run_cmd_owned<I>(dir: &Path, program: &str, args: I) -> Result<ExitStatus, String>
where
    I: IntoIterator<Item = String>,
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
        return Err(format!(
            "rustup toolchain list failed with status {}",
            out.status
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout.lines().any(|line| line.starts_with(name)))
}
