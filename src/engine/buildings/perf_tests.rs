use super::*;
#[test]
#[ignore = "manual canonical-map building authoring timing"]
fn canonical_building_authoring_timings() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl");
    let directory = std::env::temp_dir().join(format!("building-timing-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("models")).unwrap();
    let path = directory.join("map.road");
    for name in ["map.road", "elevation.png"] {
        std::fs::copy(base.join(name), directory.join(name)).unwrap();
    }
    for entry in std::fs::read_dir(base.join("models")).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_file() {
            std::fs::copy(
                entry.path(),
                directory.join("models").join(entry.file_name()),
            )
            .unwrap();
        }
    }
    let start = std::time::Instant::now();
    let mut hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
    eprintln!("initial map compile: {:?}", start.elapsed());
    let player = Vec3::new(2.1, -0.9, 0.122);
    let start = std::time::Instant::now();
    let mut doc = Document::open(&path, player, 0.3).unwrap();
    eprintln!(
        "open building tool: {:?} ({} editable buildings)",
        start.elapsed(),
        doc.items.len()
    );
    let start = std::time::Instant::now();
    let candidates = doc.candidates(hermes.city());
    eprintln!("candidate generation: {:?}", start.elapsed());
    let feature = doc.prototype(hermes.city(), candidates[0].points.clone());
    let start = std::time::Instant::now();
    doc.create(hermes.city(), feature).unwrap();
    eprintln!("create and save: {:?}", start.elapsed());
    let source = std::fs::read(&path).unwrap();
    let elevation = std::fs::read(directory.join("elevation.png")).unwrap();
    let start = std::time::Instant::now();
    hermes =
        crate::hermes::Hermes::from_map_bytes(&source, &elevation, None, Some(&directory)).unwrap();
    eprintln!("edited map compile: {:?}", start.elapsed());
    let start = std::time::Instant::now();
    hermes.prepare_for_reload();
    eprintln!("geometry/minimap/cache preparation: {:?}", start.elapsed());
    std::fs::remove_dir_all(directory).unwrap();
}
