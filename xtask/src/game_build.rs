//! Visible progress for Moon's incremental native game build.
use std::{
    collections::HashMap,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::Result;

pub fn build(root: &Path) -> Result<()> {
    eprintln!("[game build] Building incremental local game (app opt=1, world/map opt=3).");
    let started = Instant::now();
    let mut child = cargo(root)
        .args([
            "build",
            "--profile",
            "dev",
            "--bin",
            "mtl_jobines",
            "--timings",
        ])
        .env("CARGO_TERM_PROGRESS_WHEN", "never")
        .spawn()?;
    let cargo_pid = child.id();
    let (done, stop) = mpsc::channel();
    let logger = thread::spawn(move || {
        let mut previous = None;
        let mut last_log = Instant::now();
        while let Err(mpsc::RecvTimeoutError::Timeout) = stop.recv_timeout(Duration::from_secs(1)) {
            let phase = Command::new("ps")
                .args(["-axo", "pid=,ppid=,args="])
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| phase(&String::from_utf8_lossy(&output.stdout), cargo_pid))
                .unwrap_or(Phase::Cargo);
            if previous.as_ref() != Some(&phase) || last_log.elapsed() >= Duration::from_secs(5) {
                eprintln!(
                    "[game build +{:.1}s] {}",
                    started.elapsed().as_secs_f32(),
                    phase.message()
                );
                previous = Some(phase);
                last_log = Instant::now();
            }
        }
    });
    let status = child.wait();
    let _ = done.send(());
    let _ = logger.join();
    let status = status?;
    if !status.success() {
        return Err(format!(
            "[game build +{:.1}s] Build failed ({status}); game was not launched.",
            started.elapsed().as_secs_f32()
        )
        .into());
    }
    eprintln!(
        "[game build +{:.1}s] Build complete. Cargo's timing report above shows time per crate.",
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    build(root)?;
    eprintln!("[game run] Launching game; Cargo will verify the build is still current.");
    let mut command = cargo(root);
    command
        .args(["run", "--profile", "dev", "--bin", "mtl_jobines", "--"])
        .args(args);
    // Cargo resolves the executable correctly even with a custom target directory.
    // Replacing xtask also preserves Ctrl-C and the game's exit status.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec().into())
    }
    #[cfg(not(unix))]
    {
        std::process::exit(command.status()?.code().unwrap_or(1));
    }
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.current_dir(root);
    command
}

#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Cargo,
    Compiler(String),
    NativeCompiler,
    Linker,
}
impl Phase {
    fn message(&self) -> String {
        match self {
            Self::Cargo => "Cargo is running: waiting for a build lock, scheduling work, or running build scripts.".into(),
            Self::Compiler(name) => format!("Compiling/optimizing {name} (rustc is active)."),
            Self::NativeCompiler => "Compiling native code (a C/C++ compiler is active).".into(),
            Self::Linker => "Linking native output (a linker process is active).".into(),
        }
    }
}

fn phase(snapshot: &str, cargo_pid: u32) -> Phase {
    let processes: HashMap<_, _> = snapshot
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let parent = fields.next()?.parse::<u32>().ok()?;
            let executable = fields.next()?;
            Some((pid, (parent, executable, fields.collect::<Vec<_>>())))
        })
        .collect();
    let mut result = Phase::Cargo;
    for (&pid, (_, executable, args)) in &processes {
        let mut ancestor = pid;
        let mut descendant = false;
        for _ in 0..processes.len() {
            let Some((parent, ..)) = processes.get(&ancestor) else {
                break;
            };
            if *parent == cargo_pid {
                descendant = true;
                break;
            }
            ancestor = *parent;
        }
        if !descendant {
            continue;
        }
        match Path::new(executable).file_name().and_then(|s| s.to_str()) {
            Some("cc" | "clang" | "gcc") if args.contains(&"-c") => {
                result = Phase::NativeCompiler;
            }
            Some(
                "cc" | "clang" | "gcc" | "ld" | "ld.lld" | "ld64.lld" | "rust-lld" | "lld"
                | "collect2",
            ) => {
                return Phase::Linker;
            }
            Some("rustc" | "rustc.exe") => {
                let name = args
                    .windows(2)
                    .find(|pair| pair[0] == "--crate-name")
                    .map_or("Rust code", |pair| pair[1]);
                let target = args
                    .windows(2)
                    .find(|pair| pair[0] == "--crate-type")
                    .map_or("", |pair| match pair[1] {
                        "bin" => " executable",
                        "lib" | "rlib" | "dylib" | "cdylib" | "proc-macro" | "staticlib" => {
                            " library"
                        }
                        _ => "",
                    });
                result = Phase::Compiler(format!("{name}{target}"));
            }
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_compiler_for_this_cargo_and_ignores_unrelated_linkers() {
        assert_eq!(
            phase("10 1 cargo build\n11 10 /toolchain/rustc --crate-name mtl_jobines src/lib.rs\n90 1 /usr/bin/ld", 10),
            Phase::Compiler("mtl_jobines".into())
        );
    }

    #[test]
    fn reports_nested_linker_before_its_waiting_rustc_parent() {
        assert_eq!(
            phase(
                "11 10 rustc --crate-name mtl_jobines\n12 11 cc -o game\n13 12 /usr/bin/ld",
                10
            ),
            Phase::Linker
        );
    }

    #[test]
    fn identifies_library_and_executable_compilation_separately() {
        assert_eq!(
            phase(
                "11 10 rustc --crate-name mtl_jobines --crate-type rlib --crate-type cdylib",
                10
            ),
            Phase::Compiler("mtl_jobines library".into())
        );
        assert_eq!(
            phase("11 10 rustc --crate-name mtl_jobines --crate-type bin", 10),
            Phase::Compiler("mtl_jobines executable".into())
        );
    }

    #[test]
    fn does_not_report_native_compilation_as_linking() {
        assert_eq!(phase("12 10 clang -c library.c", 10), Phase::NativeCompiler);
    }

    #[test]
    fn inaccessible_process_list_and_idle_cargo_have_an_honest_fallback() {
        assert_eq!(phase("", 10), Phase::Cargo);
        assert_eq!(
            phase("10 1 cargo build\n20 1 rustc --crate-name other", 10),
            Phase::Cargo
        );
    }
}
