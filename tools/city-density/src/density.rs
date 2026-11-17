//! Offline lot packing. The result is ordinary .road geometry, never runtime fill.
use opencity::{
    heightmap::{ElevationMap, ElevationMapSpec},
    Argument, LandscapeKind, Statement,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    error::Error,
    fmt::Write,
    fs,
};
mod blocks;
mod green;
mod infill;
mod junctions;
mod plan;
pub(crate) use green::export as export_green;
use plan::Plan;
type Polygon = Vec<[f32; 2]>;
const CELL: f32 = 0.5;
const DENSITY_CENTER: [f32; 2] = [46., 10.];
fn generated(name: &str) -> bool {
    name.starts_with("generated_area_")
        || name.starts_with("generated_home_zone_")
        || name.starts_with("generated_shop_")
}
fn fixed_content(program: &opencity::Program, removed: &HashSet<String>) -> String {
    let mut fixed = program.clone();
    fixed.statements.retain(|s| {
        let Statement::Call(c) = s else { return true };
        if c.assigns
            .iter()
            .any(|n| generated(n) || removed.contains(n))
        {
            return false;
        }
        if c.name == "elevation" {
            if let Some(Argument::Identifier(n)) = c.args.first() {
                return !generated(n) && !removed.contains(n);
            }
        }
        true
    });
    fixed.to_road_source()
}

fn keys(q: &[[f32; 2]]) -> Vec<(i32, i32)> {
    let b = q.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        },
    );
    let mut out = Vec::new();
    for x in (b[0] / CELL).floor() as i32..=(b[2] / CELL).floor() as i32 {
        for y in (b[1] / CELL).floor() as i32..=(b[3] / CELL).floor() as i32 {
            out.push((x, y));
        }
    }
    out
}
#[derive(Default)]
struct Index {
    polygons: Vec<Polygon>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}
impl Index {
    fn insert(&mut self, q: Polygon) -> usize {
        let id = self.polygons.len();
        for k in keys(&q) {
            self.cells.entry(k).or_default().push(id);
        }
        self.polygons.push(q);
        id
    }
    fn update(&mut self, id: usize, q: Polygon) {
        for k in keys(&self.polygons[id]) {
            if let Some(ids) = self.cells.get_mut(&k) {
                ids.retain(|&i| i != id);
            }
        }
        for k in keys(&q) {
            self.cells.entry(k).or_default().push(id);
        }
        self.polygons[id] = q;
    }
    fn overlaps(&self, q: &[[f32; 2]], skip: Option<usize>, concave: bool) -> bool {
        keys(q).iter().any(|k| {
            self.cells.get(k).is_some_and(|ids| {
                ids.iter().any(|&id| {
                    Some(id) != skip
                        && if concave {
                            green::overlap(q, &self.polygons[id])
                        } else {
                            opencity::runtime::scaffold::overlap(q, &self.polygons[id])
                        }
                })
            })
        })
    }
}
fn area(q: &[[f32; 2]]) -> f64 {
    q.iter()
        .zip(q.iter().cycle().skip(1))
        .map(|(a, b)| a[0] as f64 * b[1] as f64 - b[0] as f64 * a[1] as f64)
        .sum::<f64>()
        .abs()
        * 0.5
}
fn supported(q: &[[f32; 2]], terrain: &ElevationMap) -> Option<(f32, f32)> {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    // Sample the full footprint, including interior water/terrain pixels.
    for x in 0..=6 {
        for y in 0..=6 {
            let u = x as f32 / 6.;
            let v = y as f32 / 6.;
            let p = [
                q[0][0] + (q[1][0] - q[0][0]) * u + (q[3][0] - q[0][0]) * v,
                q[0][1] + (q[1][1] - q[0][1]) * u + (q[3][1] - q[0][1]) * v,
            ];
            let h = terrain.surface_height_at(p)?;
            if h <= 0.002 {
                return None;
            }
            low = low.min(h);
            high = high.max(h);
        }
    }
    // A stepped foundation follows the authored city grades without changing terrain.
    // Its shell extends from the lowest ground to above the highest sample.
    (high - low <= 0.06).then_some((low, high))
}
pub fn build(base: &str) -> Result<(), Box<dyn Error>> {
    run(base, true, Plan::SourceCity)
}
pub fn fill(base: &str) -> Result<(), Box<dyn Error>> {
    run(base, false, Plan::SourceCity)
}
pub fn inner_loop(base: &str) -> Result<(), Box<dyn Error>> {
    run(base, false, Plan::InnerLoop)
}
fn run(base: &str, pack: bool, plan: Plan) -> Result<(), Box<dyn Error>> {
    let original = fs::read_to_string(format!("{base}/before.road"))?;
    let mut p = opencity::parse(&original)?;
    let terrain = ElevationMap::from_png(
        ElevationMapSpec::from_program(&p)?.ok_or("missing elevation")?,
        &fs::read(format!("{base}/elevation.png"))?,
    )?;
    let green = green::extract(base)?;
    eprintln!("Loaded {} green-space boundaries", green.polygons.len());
    let features = opencity::project_landscape_features(&p);
    if !features.errors.is_empty() {
        return Err("invalid input buildings".into());
    }
    let removed: HashSet<_> = features
        .features
        .iter()
        .filter(|f| {
            f.kind == LandscapeKind::Building
                && (plan == Plan::InnerLoop || f.name.as_deref().is_some_and(generated))
                && green.overlaps(&f.points, None, true)
        })
        .filter_map(|f| f.name.clone())
        .collect();
    // A reusable block can contain named buildings as well as top-level calls.
    for statement in &mut p.statements {
        if let Statement::Block(block) = statement {
            block.calls.retain(|c| {
                !c.assigns.iter().any(|n| removed.contains(n))
                    && !(c.name == "elevation"
                        && matches!(c.args.first(),
                        Some(Argument::Identifier(n)) if removed.contains(n)))
            });
        }
    }
    let fixed = fixed_content(&p, &removed);
    let mut index = Index::default();
    let mut lots = Vec::new();
    for f in &features.features {
        if f.kind != LandscapeKind::Building || f.name.as_ref().is_some_and(|n| removed.contains(n))
        {
            continue;
        }
        let id = index.insert(f.points.clone());
        if f.name
            .as_deref()
            .is_some_and(|n| n.starts_with("generated_area_"))
        {
            lots.push((id, f.clone()));
        }
    }
    let road_start = index.polygons.len();
    if plan == Plan::InnerLoop {
        // Keep parking, pedestrian lanes and station roofs open at street level.
        plan::reserve_fixtures(&p, &mut index);
        for f in &features.features {
            if f.kind == LandscapeKind::Concrete {
                index.insert(f.points.clone());
            }
        }
    }
    let projection = opencity::map::project(&p);
    let widths: BTreeMap<_, _> = p
        .statements
        .iter()
        .filter_map(|s| match s {
            Statement::Road(r) => Some((
                r.name.clone(),
                r.elements
                    .iter()
                    .map(|e| opencity::map::element_world_width(e.kind))
                    .sum::<f32>(),
            )),
            _ => None,
        })
        .collect();
    junctions::reserve(&p, &projection, &mut index, &widths, Some(&terrain))?;
    for r in &projection.segments {
        let length = [r.start, r.controls[0], r.controls[1], r.end]
            .windows(2)
            .map(|v| (v[1].x - v[0].x).hypot(v[1].y - v[0].y))
            .sum::<f32>();
        let count = (length / 0.025).ceil().max(2.) as usize;
        for i in 0..count {
            let t = (i as f32 + 0.5) / count as f32;
            let a = r.point_at(i as f32 / count as f32);
            let b = r.point_at((i + 1) as f32 / count as f32);
            let c = r.point_at(t);
            if r.height_at(t) < terrain.surface_height_at([c.x, c.y]).unwrap_or(0.) - 0.05 {
                continue;
            }
            let d = [b.x - a.x, b.y - a.y];
            let len = d[0].hypot(d[1]).max(1e-7);
            let half = widths[&r.road_name] / 2. + 0.025;
            let n = [-d[1] / len * half, d[0] / len * half];
            index.insert(vec![
                [a.x + n[0], a.y + n[1]],
                [b.x + n[0], b.y + n[1]],
                [b.x - n[0], b.y - n[1]],
                [a.x - n[0], a.y - n[1]],
            ]);
        }
    }
    let mut before = 0.;
    let mut after = 0.;
    let mut grown = 0;
    let mut terrain_skips = 0;
    let mut replacements = HashMap::new();
    eprintln!(
        "Packing {} lots; {} buildings removed from protected green space",
        lots.len(),
        removed.len()
    );
    for (lot_index, (id, f)) in lots.iter().enumerate() {
        if lot_index % 10000 == 0 {
            eprintln!("Packing lot {lot_index}/{}", lots.len());
        }
        let mut q = f.points.clone();
        before += area(&q);
        if !pack {
            after += area(&q);
            continue;
        }
        if supported(&q, &terrain).is_none() {
            terrain_skips += 1;
            after += area(&q);
            continue;
        }
        // The existing rectangle defines local facade/depth axes. Grow each side
        // independently until the next neighbour, park boundary or road reservation.
        let width = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
        let depth = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
        let u = [(q[1][0] - q[0][0]) / width, (q[1][1] - q[0][1]) / width];
        let v = [(q[3][0] - q[0][0]) / depth, (q[3][1] - q[0][1]) / depth];
        for step in [0.01, 0.002, 0.0005] {
            for _ in 0..20 {
                let mut changed = false;
                for (vertices, direction) in [
                    ([1, 2], u),
                    ([0, 3], [-u[0], -u[1]]),
                    ([2, 3], v),
                    ([0, 1], [-v[0], -v[1]]),
                ] {
                    let mut candidate = q.clone();
                    for vertex in vertices {
                        candidate[vertex][0] += direction[0] * step;
                        candidate[vertex][1] += direction[1] * step;
                    }
                    let w = (candidate[1][0] - candidate[0][0])
                        .hypot(candidate[1][1] - candidate[0][1]);
                    let d = (candidate[3][0] - candidate[0][0])
                        .hypot(candidate[3][1] - candidate[0][1]);
                    if w > 0.42
                        || d > 0.38
                        || index.overlaps(&candidate, Some(*id), false)
                        || green.overlaps(&candidate, None, true)
                        || supported(&candidate, &terrain).is_none()
                    {
                        continue;
                    }
                    q = candidate;
                    changed = true;
                }
                if !changed {
                    break;
                }
            }
        }
        let (low, high) = supported(&q, &terrain).ok_or("existing lot has unsupported terrain")?;
        if area(&q) > area(&f.points) + 1e-7 {
            grown += 1;
        } else {
            after += area(&q);
            continue;
        }
        // Keep a five-centimetre inset for stable serialized collision geometry.
        // Growth tests operate before decimal serialization and f32 projection.
        for (i, (su, sv)) in [(1., 1.), (-1., 1.), (-1., -1.), (1., -1.)]
            .into_iter()
            .enumerate()
        {
            q[i][0] += 0.0005 * (su * u[0] + sv * v[0]);
            q[i][1] += 0.0005 * (su * u[1] + sv * v[1]);
        }
        after += area(&q);
        index.update(*id, q.clone());
        let xy = q
            .iter()
            .map(|p| format!("{:.7},{:.7}", p[0], p[1]))
            .collect::<Vec<_>>()
            .join(",");
        let style = f.building_style.ok_or("missing building style")?;
        let original_high = f.elevation + f.height.ok_or("missing height")?;
        let new_height = original_high.max(high + 0.05) - (low - 0.002);
        let name = f.name.as_ref().ok_or("missing building name")?;
        let mut source = format!(
            "{}=building({:.7},{{use:{},material:{}}},{});",
            name,
            new_height,
            f.building_use.keyword(),
            style.material.keyword(),
            xy
        );
        write!(source, " elevation({name},{:.7});", low - 0.002)?;
        replacements.insert(name.clone(), opencity::parse(&source)?.statements);
    }
    // Independent final spatial audit against every building and road reservation.
    let conflicts = lots
        .iter()
        .filter(|(id, _)| index.overlaps(&index.polygons[*id], Some(*id), false))
        .count();
    if conflicts != 0 {
        return Err(format!("{conflicts} building/road overlaps").into());
    }
    eprintln!("Adding street rows after packing {grown} lots");
    let (infill, infill_count) = infill::build(
        &p,
        &terrain,
        &mut index,
        &green,
        plan,
        if pack { removed.len() + 1500 } else { 10000 },
    )?;
    let budget = if pack { removed.len() + 1500 } else { 10000 };
    let (blocks, block_count) = blocks::build(
        &p,
        &terrain,
        &mut index,
        &green,
        plan,
        budget.saturating_sub(infill_count),
    )?;
    let mut output = Vec::new();
    for s in p.statements {
        if let Statement::Call(c) = &s {
            if c.assigns.iter().any(|n| removed.contains(n)) {
                continue;
            }
            if c.name == "elevation" {
                if let Some(Argument::Identifier(n)) = c.args.first() {
                    if removed.contains(n) || replacements.contains_key(n) {
                        continue;
                    }
                }
            }
            if let Some(name) = c.assigns.iter().find(|n| replacements.contains_key(*n)) {
                output.extend(replacements[name].clone());
                continue;
            }
        }
        output.push(s);
    }
    output.extend(infill);
    output.extend(blocks);
    p.statements = output;
    let source = p.to_road_source();
    let checked = opencity::parse(&source)?;
    if fixed_content(&checked, &removed) != fixed {
        return Err("fixed map content changed".into());
    }
    let checked = opencity::project_landscape_features(&checked);
    if !checked.errors.is_empty() {
        return Err("invalid final geometry".into());
    }
    let mut final_index = Index::default();
    let mut audited = Vec::new();
    let road_end = index.polygons.len() - infill_count - block_count;
    for q in &index.polygons[road_start..road_end] {
        final_index.insert(q.clone());
    }
    for f in &checked.features {
        if f.kind != LandscapeKind::Building {
            continue;
        }
        let id = final_index.insert(f.points.clone());
        if plan == Plan::InnerLoop || f.name.as_deref().is_some_and(generated) {
            if green.overlaps(&f.points, None, true) {
                return Err("final building intrudes into green space".into());
            }
            if f.name
                .as_deref()
                .is_some_and(|n| n.starts_with("generated_area_"))
            {
                audited.push(id);
            }
        }
    }
    let final_conflicts = audited
        .iter()
        .filter(|&&id| final_index.overlaps(&final_index.polygons[id], Some(id), false))
        .count();
    if final_conflicts != 0 {
        return Err(format!("{final_conflicts} final building/road overlaps").into());
    }
    fs::write(format!("{base}/dense.road"), source)?;
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut infill_area = 0.;
    let mut block_area = 0.;
    let mut generated_area = 0.;
    for f in checked
        .features
        .iter()
        .filter(|f| f.kind == LandscapeKind::Building)
    {
        let kind = if f.model.is_some() {
            "landmark_or_foundation"
        } else {
            f.building_use.keyword()
        };
        *counts.entry(kind).or_default() += 1;
        if let Some(n) = f.name.as_deref() {
            if n.starts_with("generated_area_infill_") {
                infill_area += area(&f.points);
            }
            if n.starts_with("generated_area_interior_") {
                block_area += area(&f.points);
            }
            if n.starts_with("generated_area_") {
                generated_area += area(&f.points);
            }
        }
    }
    let report = json!({
        "expanded_buildings":grown,
        "retained_developed_buildings":lots.len(),
        "building_counts":counts,
        "infill_footprint_area":infill_area,
        "block_footprint_area":block_area,
        "total_generated_footprint_area":generated_area,
        "new_street_row_buildings":infill_count,
        "new_block_buildings":block_count,
        "final_building_road_overlaps":final_conflicts,
        "final_generated_buildings_in_green_space":0,
        "non_generated_map_content":"verified unchanged except explicit park removals",
        "removed_park_building_names": removed.iter().collect::<std::collections::BTreeSet<_>>(),
        "existing_lots_left_for_terrain":terrain_skips,
        "removed_park_buildings":removed.len(),
        "protected_green_polygons":green.polygons.len(),
        "developed_footprint_area_before":before,
        "developed_footprint_area_after":after,
        "footprint_area_increase_percent":(after/before-1.)*100.,
        "building_road_overlaps":conflicts,
        "road_reservations":road_end-road_start,
        "terrain_and_roads":"unchanged"
    });
    fs::write(
        format!("{base}/report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terrain(pixels: &[u8], heights: [f32; 2]) -> ElevationMap {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1024, 1024);
            encoder.set_color(png::ColorType::Grayscale);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(pixels)
                .unwrap();
        }
        ElevationMap::from_png(
            ElevationMapSpec {
                path: String::new(),
                origin: [-1., -1.],
                size: [2., 2.],
                heights,
                water_level: Some(0.),
                cuts: Vec::new(),
            },
            &bytes,
        )
        .unwrap()
    }
    #[test]
    fn city_grade_accepts_a_fitted_stepped_foundation() {
        let pixels: Vec<_> = (0..1024 * 1024)
            .map(|i| ((i % 1024) * 255 / 1023) as u8)
            .collect();
        let terrain = terrain(&pixels, [0.1, 0.3]);
        let q = [[-0.2, -0.1], [0.2, -0.1], [0.2, 0.1], [-0.2, 0.1]];
        let (low, high) =
            supported(&q, &terrain).expect("a four-metre foundation step is buildable");
        assert!((high - low - 0.04).abs() < 0.002);
    }
    #[test]
    fn an_interior_water_pixel_rejects_a_lot_even_when_all_corners_are_dry() {
        let mut pixels = vec![128; 1024 * 1024];
        for y in 510..=513 {
            for x in 510..=513 {
                pixels[y * 1024 + x] = 0;
            }
        }
        let terrain = terrain(&pixels, [-0.1, 0.3]);
        let q = [[-0.2, -0.1], [0.2, -0.1], [0.2, 0.1], [-0.2, 0.1]];
        assert!(q
            .iter()
            .all(|&p| terrain.surface_height_at(p).unwrap() > 0.002));
        assert!(supported(&q, &terrain).is_none());
    }
}
