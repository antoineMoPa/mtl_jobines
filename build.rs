use std::{fs, path::Path};

fn check_commerce_assets() {
    let mut missing = Vec::new();
    for directory in ["assets/commerce", "assets/commerce/brands"] {
        for entry in fs::read_dir(directory).expect("commerce SVG sources must exist") {
            let source = entry.expect("commerce SVG source must be readable").path();
            if source
                .extension()
                .is_some_and(|extension| extension == "svg")
            {
                let png = source.with_extension("png");
                if !png.is_file() {
                    missing.push(png);
                }
            }
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "Missing generated commerce PNGs ({} files).\n\nRun `cargo xtask assets` from the repository root, then retry your Cargo command.\nAsset generation requires Georgia and Arial fonts.\nFirst missing asset: {}",
        missing.len(),
        missing.first().map_or_else(String::new, |path| path.display().to_string()),
    );
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
}
fn hash_sources(path: &Path, hash: &mut u64) {
    let mut paths: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            hash_sources(&path, hash);
        } else if path.extension().is_some_and(|ext| {
            matches!(
                ext.to_str(),
                Some("rs" | "wgsl" | "road" | "png" | "svg" | "json" | "mesh" | "glb" | "ttf")
            )
        }) {
            hash_bytes(hash, &fs::read(path).unwrap());
        }
    }
}
fn main() {
    // This script belongs to mtl_world, whose manifest is below the repo root.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::set_current_dir(&root).expect("workspace root must exist");
    println!("cargo:rustc-env=MTL_WORKSPACE_DIR={}", root.display());

    println!(
        "cargo:rerun-if-changed={}",
        root.join("opencity/src").display()
    );
    println!("cargo:rerun-if-changed={}", root.join("assets").display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("data/landmarks").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("Cargo.lock").display()
    );
    println!("cargo:rerun-if-changed={}", root.join("build.rs").display());
    check_commerce_assets();
    let mut hash = 0xcbf29ce484222325;
    // Renderer/UI edits cannot affect generated map data or this library.
    for source in [
        "src/world.rs",
        "src/audio",
        "src/dynamic_world",
        "src/hermes",
        "src/kairos",
        "src/performance.rs",
        "src/props",
        "src/season.rs",
        "src/web_assets.rs",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(source).display());
        let path = Path::new(source);
        if path.is_dir() {
            hash_sources(path, &mut hash);
        } else {
            hash_bytes(&mut hash, &fs::read(path).unwrap());
        }
    }
    hash_sources(Path::new("assets"), &mut hash);
    hash_sources(Path::new("data/landmarks"), &mut hash);
    hash_sources(Path::new("opencity/src"), &mut hash);
    println!(
        "cargo:rerun-if-changed={}",
        root.join("opencity/assets").display()
    );
    hash_sources(Path::new("opencity/assets"), &mut hash);
    hash_bytes(&mut hash, &fs::read("Cargo.lock").unwrap());
    hash_bytes(&mut hash, &fs::read("build.rs").unwrap());
    println!("cargo:rustc-env=MTL_JOBINES_GENERATOR_KEY={hash:016x}");
    println!("cargo:rustc-check-cfg=cfg(bundled_game_data)");
    println!("cargo:rerun-if-env-changed=MTL_JOBINES_BUNDLED_GAME_DATA");
    if let Some(path) = std::env::var_os("MTL_JOBINES_BUNDLED_GAME_DATA") {
        let path = fs::canonicalize(path)
            .expect("bundled game data must exist; generate it with --build-game-data");
        println!("cargo:rerun-if-changed={}", path.display());
        println!(
            "cargo:rustc-env=MTL_JOBINES_BUNDLED_GAME_DATA={}",
            path.display()
        );
        println!("cargo:rustc-cfg=bundled_game_data");
    }
}
