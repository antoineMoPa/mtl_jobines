//! Reconstruct editor-compatible JSON from the compact map PNGs for authoring tools.
use mtl_world::hermes::ski_hill::{read_area_png, read_density_png};
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new("data/maps/mtl");
    let mut design = serde_json::json!({
        "format": "ski-hill-study",
        "version": 1,
        "name": "Ski hill",
        "notes": "",
        "reference": "data/maps/mtl/ski-hill-areas.png; ski-hill-tree-density.png",
        "grid": {
            "columns": 180,
            "rows": 190,
            "cellMetres": 10,
            "originMap": [-10, -76],
            "rowDirection": "negative-y"
        },
        "legend": {"0": "unmarked", "1": "spruce-forest", "2": "skiable", "3": "individual-spruce"}
    });
    design["cells"] =
        serde_json::to_value(read_area_png(&fs::read(root.join("ski-hill-areas.png"))?)?)?;
    design["densities"] = serde_json::to_value(read_density_png(&fs::read(
        root.join("ski-hill-tree-density.png"),
    )?)?)?;
    println!("{}", serde_json::to_string(&design)?);
    Ok(())
}
