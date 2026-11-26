//! Sample the actual game PNG, without altering terrain, for route/contour inspection.
use opencity::heightmap::ElevationMapSpec;
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string("data/maps/mtl/map.road")?;
    let program = opencity::parse(&source).map_err(|e| e.to_string())?;
    let spec = ElevationMapSpec::from_program(&program)?.ok_or("missing elevation map")?;
    let map = spec.load(Some(Path::new("data/maps/mtl")))?;
    let sample = |c: f32, r: f32| map.height_at([-10. + c * 0.1, -76. - r * 0.1]).unwrap() * 100.;
    let heights: Vec<_> = (0..191)
        .flat_map(|r| (0..181).map(move |c| (c, r)))
        .map(|(c, r)| sample(c as f32, r as f32))
        .collect();
    let cells: Vec<_> = (0..190)
        .flat_map(|r| (0..180).map(move |c| (c, r)))
        .map(|(c, r)| sample(c as f32 + 0.5, r as f32 + 0.5))
        .collect();
    let result = serde_json::json!({"source":"elevation.png","originMap":[-10.,-76.],"stepMetres":10,"columns":181,"rows":191,"heightsMetres":heights,"cellHeightsMetres":cells});
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/maps/mtl/ski-hill-generated/ski-hill-elevation.json".into());
    if let Some(parent) = Path::new(&path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec(&result)?)?;
    println!("Sampled actual PNG terrain into {path}; no terrain edits");
    Ok(())
}
