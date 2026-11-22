//! Visible progress for Moon's incremental native game build.
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::Result;

pub fn build(root: &Path) -> Result<()> {
    build_once(root).map(|_| ())
}

fn build_once(root: &Path) -> Result<PathBuf> {
    build_command(cargo(root))
}

fn build_command(mut command: Command) -> Result<PathBuf> {
    eprintln!("[game build] Building incremental local game (app opt=1, world/map opt=3).");
    let started = Instant::now();
    let mut child = command
        .args([
            "build",
            "--profile",
            "dev",
            "--bin",
            "mtl_jobines",
            "--timings",
            "--message-format=json-render-diagnostics",
        ])
        .env("CARGO_TERM_PROGRESS_WHEN", "never")
        .stdout(Stdio::piped())
        .spawn()?;
    let output = child.stdout.take().ok_or("missing Cargo stdout")?;
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
    // Drain stdout while Cargo runs so its message pipe cannot fill up.
    let executable = executable(BufReader::new(output));
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
        "[game build +{:.1}s] Cargo pass complete. Cargo's timing report above shows time per crate.",
        started.elapsed().as_secs_f32()
    );
    executable?.ok_or_else(|| "Cargo did not report the game executable".into())
}

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let executable = build_once(root)?;
    eprintln!("[game run] Launching game.");
    let mut command = Command::new(executable);
    command.current_dir(root).args(args);
    // Cargo's artifact message resolves custom target directories and targets.
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

fn executable(output: impl BufRead) -> Result<Option<PathBuf>> {
    let mut executable = None;
    for line in output.lines() {
        let line = line?;
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) else {
            println!("{line}");
            continue;
        };
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == "mtl_jobines"
            && message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        {
            if let Some(path) = message["executable"].as_str() {
                executable = Some(PathBuf::from(path));
            }
        }
    }
    Ok(executable)
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    // Keep symbols in a dSYM so repeated macOS links can discard loose objects.
    #[cfg(target_os = "macos")]
    if std::env::var_os("CARGO_PROFILE_DEV_SPLIT_DEBUGINFO").is_none() {
        command.env("CARGO_PROFILE_DEV_SPLIT_DEBUGINFO", "packed");
    }
    // Agent checks/tests and editor builds keep the ordinary cache. The game
    // launcher gets a separate Cargo lock and feature/profile artifact cache.
    let target = root
        .join(std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()))
        .join("moon-game");
    command.current_dir(root).env("CARGO_TARGET_DIR", target);
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
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Inputs(PathBuf);

    impl Inputs {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "game-build-inputs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, file: &str, contents: &str) {
            let path = self.0.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for Inputs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn selects_game_binary_from_cargo_messages_including_fresh_artifacts() {
        let messages = concat!(
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"mtl_jobines\",\"kind\":[\"lib\"]},\"executable\":null}\n",
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"other\",\"kind\":[\"bin\"]},\"executable\":\"/wrong\"}\n",
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"mtl_jobines\",\"kind\":[\"bin\"]},\"executable\":\"/custom target/debug/game\",\"fresh\":true}\n",
            "{\"reason\":\"build-finished\",\"success\":true}\n",
        );
        assert_eq!(
            executable(messages.as_bytes()).unwrap(),
            Some(PathBuf::from("/custom target/debug/game"))
        );
        assert_eq!(executable("".as_bytes()).unwrap(), None);
    }

    #[test]
    fn real_cargo_reports_a_runnable_artifact_after_one_build() {
        let inputs = Inputs::new();
        inputs.write("Cargo.toml", "[package]\nname = \"mtl_jobines\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n");
        inputs.write(
            "src/main.rs",
            r#"
            fn main() {
                println!("game {:?}", std::env::args().skip(1).collect::<Vec<_>>());
                std::process::exit(7);
            }
        "#,
        );
        let mut command = cargo(&inputs.0);
        // Keep this fixture isolated even when the caller sets CARGO_TARGET_DIR.
        command.env("CARGO_TARGET_DIR", inputs.0.join("custom target"));
        let path = build_command(command).unwrap();
        let output = Command::new(path)
            .args(["--map", "a map.road"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "game [\"--map\", \"a map.road\"]\n"
        );
    }

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
