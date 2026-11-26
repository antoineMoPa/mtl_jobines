//! Build and publish the browser game as a static Pages site.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::Result;

const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const WEB_SOURCES: &[&str] = &[
    "index.html",
    "game/index.html",
    "intro/intro.css",
    "intro/language.js",
    "intro/gallery.js",
    "intro/orange-julep.webp",
    "intro/opencity-routes.webp",
    "intro/mtl-objets.webp",
    "intro/claydash-deriveur.webp",
    "intro/centre-ville.webp",
    "intro/aeroport.webp",
    "intro/survol-mtl.webp",
    "intro/plex.webp",
    "intro/lampadaire.webp",
    "intro/taxi.webp",
    "intro/snowplow.webp",
    "intro/acadie.webp",
    "intro/mode-zombie.webp",
    "intro/carte-montreal.webp",
    "boot.png",
    "mobile-controls.js",
    "mobile-controls.css",
];
const REQUIRED: &[&str] = &[
    "assets/fonts/pt-sans-bold.ttf",
    "assets/fonts/OFL.txt",
    "pkg/mtl_jobines.js",
    "pkg/mtl_jobines_bg.wasm",
];
const HIDDEN_BUG_UI: &[&[u8]] = &[b"[bug]", b"Report a bug", b"bug.report", b"bug_report_link"];

pub fn check(root: &Path) -> Result<()> {
    let dist = root.join("web/dist");
    if !dist.is_dir() {
        return Err("Web build is missing; run make build first".into());
    }
    for file in WEB_SOURCES {
        let built = dist.join(file);
        if !built.is_file() || fs::read(root.join("web").join(file))? != fs::read(built)? {
            return Err(format!("Web release is missing or has an outdated copy of {file}").into());
        }
    }
    for file in REQUIRED {
        if !dist.join(file).is_file() {
            return Err(format!("Web build is missing {file}").into());
        }
    }
    let manifest = fs::read_to_string(root.join("web/assets.list"))?;
    for path in manifest.lines() {
        if path.is_empty() || path.starts_with('/') || path.split('/').any(|part| part == "..") {
            return Err(format!("Invalid web asset path: {path}").into());
        }
        let source = root.join(path);
        let deployed = dist.join(path);
        if !source.is_file() || !deployed.is_file() {
            return Err(format!("Web asset is missing: {path}").into());
        }
        if fs::read(source)? != fs::read(deployed)? {
            return Err(format!("Web asset differs from source: {path}").into());
        }
    }

    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut stack = vec![dist.clone()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(
                    format!("Web build contains a symlink: {}", entry.path().display()).into(),
                );
            }
            if kind.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if !kind.is_file() {
                return Err(
                    format!("Unexpected web build entry: {}", entry.path().display()).into(),
                );
            }
            let size = entry.metadata()?.len();
            if size > MAX_FILE_BYTES {
                return Err(format!(
                    "Pages file limit exceeded: {} has {size} bytes (max {MAX_FILE_BYTES})",
                    entry.path().display()
                )
                .into());
            }
            files += 1;
            bytes += size;
        }
    }
    if files > MAX_FILES {
        return Err(format!("Pages file limit exceeded: {files} files (max {MAX_FILES})").into());
    }
    for file in [
        "index.html",
        "game/index.html",
        "pkg/mtl_jobines.js",
        "pkg/mtl_jobines_bg.wasm",
    ] {
        let content = fs::read(dist.join(file))?;
        for phrase in HIDDEN_BUG_UI {
            if content
                .windows(phrase.len())
                .any(|window| window == *phrase)
            {
                return Err(
                    format!("Browser build contains native bug-report UI in {file}").into(),
                );
            }
        }
    }
    println!("Web release ready: {files} files, {bytes} bytes; each file fits Pages Free.");
    Ok(())
}

fn project_name() -> String {
    env::var("PAGES_PROJECT").unwrap_or_else(|_| "mtl-jobines".to_owned())
}

fn production_branch() -> String {
    env::var("PAGES_BRANCH").unwrap_or_else(|_| "main".to_owned())
}

fn has_supported_node_version(binary: &Path) -> bool {
    let Ok(output) = Command::new(binary).arg("--version").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let Ok(version) = std::str::from_utf8(&output.stdout) else {
        return false;
    };
    version
        .trim()
        .strip_prefix('v')
        .and_then(|version| version.split('.').next())
        .and_then(|major| major.parse::<u32>().ok())
        .is_some_and(|major| major >= 22)
}

fn compatible_node() -> Option<PathBuf> {
    let search_path = env::var_os("PATH").unwrap_or_default();
    for directory in env::split_paths(&search_path) {
        let binary = directory.join("node");
        if has_supported_node_version(&binary) {
            return Some(binary);
        }
    }
    let nvm_roots = [
        env::var_os("NVM_DIR").map(PathBuf::from),
        env::var_os("HOME").map(|home| PathBuf::from(home).join(".nvm")),
    ];
    for root in nvm_roots.into_iter().flatten() {
        if let Ok(versions) = fs::read_dir(root.join("versions/node")) {
            for version in versions.flatten() {
                let binary = version.path().join("bin/node");
                if has_supported_node_version(&binary) {
                    return Some(binary);
                }
            }
        }
    }
    for binary in [
        "/opt/homebrew/opt/node/bin/node",
        "/usr/local/opt/node/bin/node",
    ] {
        let binary = PathBuf::from(binary);
        if has_supported_node_version(&binary) {
            return Some(binary);
        }
    }
    None
}

fn command(root: &Path, program: &str, uses_node: bool) -> Result<Command> {
    let mut command = Command::new(program);
    command.current_dir(root);
    if env::var_os("WRANGLER_LOG_PATH").is_none() {
        command.env(
            "WRANGLER_LOG_PATH",
            env::temp_dir().join("mtl-jobines-wrangler-logs"),
        );
    }
    let node_binary = match env::var_os("NODE_BIN") {
        Some(binary) => Some(PathBuf::from(binary)),
        None if uses_node => Some(
            compatible_node()
                .ok_or("Wrangler requires Node.js 22 or newer; install it or set NODE_BIN")?,
        ),
        None => None,
    };
    if let Some(node_binary) = node_binary {
        if !node_binary.is_absolute() || !node_binary.is_file() {
            return Err("NODE_BIN must be the absolute path to a Node executable".into());
        }
        let previous = env::var_os("PATH").unwrap_or_default();
        let search_path = env::join_paths(
            std::iter::once(node_binary.parent().unwrap().to_path_buf())
                .chain(env::split_paths(&previous)),
        )?;
        command.env("PATH", search_path);
    }
    Ok(command)
}

fn run(mut command: Command, args: &[&str]) -> Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .args(args)
        .status()
        .map_err(|error| format!("Could not run {program}: {error}"))?;
    if !status.success() {
        return Err(format!("{program} failed with {status}").into());
    }
    Ok(())
}

fn wrangler(root: &Path) -> Result<Command> {
    let mut npx = command(root, "npx", true)?;
    npx.args(["--yes", "wrangler"]);
    Ok(npx)
}

pub fn create_project(root: &Path) -> Result<()> {
    let project = project_name();
    let branch = format!("--production-branch={}", production_branch());
    run(
        wrangler(root)?,
        &["pages", "project", "create", &project, &branch],
    )
}

pub fn deploy(root: &Path, dry_run: bool) -> Result<()> {
    let mut uploader = wrangler(root)?;
    let mut version_check = command(root, &uploader.get_program().to_string_lossy(), true)?;
    version_check.args(uploader.get_args());
    run(version_check, &["--version"])?;
    run(command(root, "bash", false)?, &["scripts/build-web.sh"])?;
    check(root)?;
    let project = project_name();
    let project_arg = format!("--project-name={project}");
    let branch_arg = format!("--branch={}", production_branch());
    let args = ["pages", "deploy", "web/dist", &project_arg, &branch_arg];
    if dry_run {
        uploader.args(args);
        println!(
            "Dry run: {:?} {:?}",
            uploader.get_program(),
            uploader.get_args().collect::<Vec<_>>()
        );
        return Ok(());
    }
    run(uploader, &args)
}
