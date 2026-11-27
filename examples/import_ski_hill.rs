//! Import the editor's exact 10 m mask and author sparse shared spruces/side lifts.
use glam::Vec2;
use mtl_world::hermes::{external_models, Hermes};
use serde::Deserialize;
use std::{fs, path::Path};
const BEGIN: &str = "# BEGIN IMPORTED SKI HILL";
const END: &str = "# END IMPORTED SKI HILL";
const MAX_TREES: usize = 512;
#[derive(Deserialize)]
struct Grid {
    columns: usize,
    rows: usize,
    #[serde(rename = "cellMetres")]
    cell_metres: u32,
    #[serde(rename = "originMap")]
    origin: [f32; 2],
    #[serde(rename = "rowDirection")]
    direction: String,
}
#[derive(Deserialize)]
struct Design {
    format: String,
    version: u32,
    grid: Grid,
    cells: Vec<u8>,
    densities: Vec<u8>,
}
impl Design {
    fn point(&self, c: usize, r: usize) -> Vec2 {
        Vec2::new(
            self.grid.origin[0] + (c as f32 + 0.5) * 0.1,
            self.grid.origin[1] - (r as f32 + 0.5) * 0.1,
        )
    }
    fn kind(&self, p: Vec2) -> u8 {
        let c = ((p.x - self.grid.origin[0]) * 10.).floor() as isize;
        let r = ((self.grid.origin[1] - p.y) * 10.).floor() as isize;
        if c < 0 || r < 0 || c >= self.grid.columns as isize || r >= self.grid.rows as isize {
            return 0;
        }
        self.cells[r as usize * self.grid.columns + c as usize]
    }
    fn near_ski(&self, wanted: Vec2, others: &[Vec2]) -> Vec2 {
        (1..self.grid.rows - 1)
            .flat_map(|r| (1..self.grid.columns - 1).map(move |c| (c, r)))
            .filter_map(|(c, r)| {
                let p = self.point(c, r);
                if self.kind(p) != 2 || others.iter().any(|o| o.distance(p) < 0.45) {
                    return None;
                }
                let wide = (-1..=1).all(|dy| {
                    (-1..=1).all(|dx| {
                        self.cells[((r as isize + dy) as usize) * self.grid.columns
                            + (c as isize + dx) as usize]
                            == 2
                    })
                });
                wide.then_some((p, p.distance_squared(wanted)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|p| p.0)
            .expect("a wide skiable station location")
    }
}
fn seed(i: usize) -> u32 {
    let mut n = (i as u32 + 1).wrapping_mul(374761393);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^ (n >> 16)
}
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    p.distance(a + d * ((p - a).dot(d) / d.length_squared()).clamp(0., 1.))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run --example import_ski_hill -- exported-study.json")?;
    let raw = fs::read(&input)?;
    let d: Design = serde_json::from_slice(&raw)?;
    if d.format != "ski-hill-study"
        || d.version != 1
        || d.grid.columns != 180
        || d.grid.rows != 190
        || d.grid.cell_metres != 10
        || d.grid.origin != [-10., -76.]
        || d.grid.direction != "negative-y"
        || d.cells.len() != 34200
        || d.densities.len() != 34200
        || d.cells.iter().any(|&v| v > 3)
        || d.densities.iter().any(|&v| v > 100)
    {
        return Err("incompatible ski hill editor export".into());
    }
    let path = Path::new("data/maps/mtl/map.road");
    let mut map = fs::read_to_string(path)?;
    if let Some(a) = map.find(BEGIN) {
        let b = map[a..].find(END).ok_or("missing imported section end")? + a + END.len();
        let start = if a > 0 && map.as_bytes()[a - 1] == b'\n' {
            a - 1
        } else {
            a
        };
        let end = b + usize::from(map.as_bytes().get(b) == Some(&b'\n'));
        map.replace_range(start..end, "");
    }
    let elevation = fs::read("data/maps/mtl/elevation.png")?;
    let h = Hermes::from_map_bytes(
        map.as_bytes(),
        &elevation,
        None,
        Some(Path::new("data/maps/mtl")),
    )?;
    let ground = |p: Vec2| {
        h.city()
            .surface_at(p.to_array(), f32::INFINITY)
            .map(|s| s.height)
            .ok_or("station/tree without ground")
    };
    let main = [Vec2::new(3.8, -82.6), Vec2::new(-1.92, -85.51)];
    let mut lifts = vec![main];
    let mut occupied = main.to_vec();
    for (name, base, top) in [("South", [-4.85, -92.05], [-2.25, -85.85])] {
        let base = d.near_ski(base.into(), &occupied);
        occupied.push(base);
        let top = d.near_ski(top.into(), &occupied);
        occupied.push(top);
        let rise = (ground(top)? - ground(base)?) * 100.;
        if rise < 5. {
            return Err(format!("{name} lift is not uphill ({rise} m)").into());
        }
        println!(
            "{name} side: ({:.2},{:.2}) -> ({:.2},{:.2}), rise {rise:.1} m",
            base.x, base.y, top.x, top.y
        );
        lifts.push([base, top]);
    }
    let mesh = external_models::decode_shared(&fs::read("data/maps/mtl/models/ski-spruce.glb")?)?;
    let (lo, hi) = external_models::bounds(&mesh);
    let size = hi - lo;
    let mut generated=format!("\n{BEGIN}\n# Imported from the ski-hill area and density PNG masks. Regenerate with the import_ski_hill example.\n# Sparse 36-triangle spruces share one GLB; cap {MAX_TREES}; approximately 30 m spacing.\nski_hill_design();\n");
    for (i, [base, top]) in lifts.iter().enumerate().skip(1) {
        generated.push_str(&format!("# Side return lift {i}; automatically boards from the loading lane.\nchairlift({:.4}, {:.4}, {:.4}, {:.4});\n",base.x,base.y,top.x,top.y));
    }
    let mut trees = Vec::new();
    for r in (1..d.grid.rows - 1).step_by(3) {
        for c in (1..d.grid.columns - 1).step_by(3) {
            let i = r * d.grid.columns + c;
            if d.cells[i] != 1 && d.cells[i] != 3 {
                continue;
            }
            let n = seed(i);
            if d.cells[i] != 3 && n % 100 >= d.densities[i] as u32 {
                continue;
            }
            let p = d.point(c, r)
                + Vec2::new(
                    ((n >> 8) % 101) as f32 / 100. - 0.5,
                    ((n >> 16) % 101) as f32 / 100. - 0.5,
                ) * 0.05;
            if (-1..=1)
                .any(|dy| (-1..=1).any(|dx| d.kind(p + Vec2::new(dx as f32, dy as f32) * 0.1) != 1))
            {
                continue;
            }
            if lifts.iter().any(|[a, b]| {
                segment_distance(p, *a, *b) < 0.12 || p.distance(*a) < 0.25 || p.distance(*b) < 0.25
            }) || p.distance(Vec2::new(4.1, -82.6)) < 0.25
            {
                continue;
            }
            if trees.len() < MAX_TREES {
                trees.push((p, 0.78 + (n % 43) as f32 / 100.));
            }
        }
    }
    for (index, (p, scale)) in trees.iter().enumerate() {
        let x = size.x * scale / 200.;
        let y = size.y * scale / 200.;
        let z = ground(*p)?;
        generated.push_str(&format!("ski_spruce_{index:04} = building({:.7}, \"models/ski-spruce.glb\", {{ collision: none }}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6});\nelevation(ski_spruce_{index:04}, {:.7});\n",size.z*scale/100.,p.x-x,p.y-y,p.x+x,p.y-y,p.x+x,p.y+y,p.x-x,p.y+y,z));
    }
    generated.push_str(&format!(
        "# {} shared spruces; {} ha pistes, {} ha forest in the exact imported mask.\n{END}\n",
        trees.len(),
        d.cells.iter().filter(|&&v| v == 2).count() as f32 / 100.,
        d.cells.iter().filter(|&&v| v == 1 || v == 3).count() as f32 / 100.
    ));
    map.push_str(&generated);
    fs::write(path, map)?;
    let (areas, density) = mtl_world::hermes::ski_hill::write_design_pngs(&d.cells, &d.densities)?;
    fs::write("data/maps/mtl/ski-hill-areas.png", areas)?;
    fs::write("data/maps/mtl/ski-hill-tree-density.png", density)?;
    let routes:Vec<_>=lifts.iter().enumerate().map(|(i,[a,b])|serde_json::json!({"id":i,"name":(["Chalet","South"][i]),"baseMap":a.to_array(),"topMap":b.to_array()})).collect();
    fs::write(
        "data/maps/mtl/ski-hill-lifts.json",
        serde_json::to_string_pretty(&routes)? + "\n",
    )?;
    println!("Imported {} piste cells, {} forest cells; {} spruces ({} triangles total, one {}-byte shared mesh); {} chairlifts",d.cells.iter().filter(|&&v|v==2).count(),d.cells.iter().filter(|&&v|v==1).count(),trees.len(),trees.len()*36,mesh.vertices.len()*std::mem::size_of::<opencity::runtime::Vertex>(),lifts.len());
    Ok(())
}
