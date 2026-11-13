use std::{fs, path::Path};
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
                Some("rs" | "wgsl" | "road" | "png" | "svg" | "json" | "mesh")
            )
        }) {
            hash_bytes(hash, &fs::read(path).unwrap());
        }
    }
}
fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=opencity/src");
    println!("cargo:rerun-if-changed=assets/default-housing.road");
    println!("cargo:rerun-if-changed=assets/commerce");
    println!("cargo:rerun-if-changed=assets/graffiti");
    println!("cargo:rerun-if-changed=data/landmarks");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=build.rs");
    let mut hash = 0xcbf29ce484222325;
    hash_sources(Path::new("src"), &mut hash);
    hash_sources(Path::new("assets"), &mut hash);
    hash_sources(Path::new("data/landmarks"), &mut hash);
    hash_sources(Path::new("opencity/src"), &mut hash);
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
