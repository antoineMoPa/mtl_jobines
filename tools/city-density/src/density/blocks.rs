//! Fill vacant block interiors near surface streets, outside protected land.
use super::{supported, Index, DENSITY_CENTER};
use opencity::{heightmap::ElevationMap, Program, Statement};
use std::{collections::BTreeMap, error::Error, fmt::Write};

pub(super) fn build(
    program: &Program,
    terrain: &ElevationMap,
    index: &mut Index,
    green: &Index,
    budget: usize,
) -> Result<(Vec<Statement>, usize), Box<dyn Error>> {
    type Sample = ([f32; 2], [f32; 2]);
    let mut cells = BTreeMap::<(i32, i32), Vec<Sample>>::new();
    for r in opencity::map::project(program)
        .segments
        .into_iter()
        .filter(|r| matches!(r.road_name.as_str(), "sd_street" | "urban_arterial"))
    {
        let length = [r.start, r.controls[0], r.controls[1], r.end]
            .windows(2)
            .map(|p| (p[1].x - p[0].x).hypot(p[1].y - p[0].y))
            .sum::<f32>();
        let count = (length / 0.2).ceil().max(1.) as usize;
        for i in 0..=count {
            let t = i as f32 / count as f32;
            let p = r.point_at(t);
            let a = r.point_at((t - 0.001).max(0.));
            let b = r.point_at((t + 0.001).min(1.));
            let length = (b.x - a.x).hypot(b.y - a.y);
            if length < 1e-7 {
                continue;
            }
            if (r.height_at(t) - terrain.surface_height_at([p.x, p.y]).unwrap_or(0.)).abs() > 0.05 {
                continue;
            }
            let direction = [(b.x - a.x) / length, (b.y - a.y) / length];
            let key = ((p.x / 0.5).floor() as i32, (p.y / 0.5).floor() as i32);
            for x in -4..=4 {
                for y in -4..=4 {
                    cells
                        .entry((key.0 + x, key.1 + y))
                        .or_default()
                        .push(([p.x, p.y], direction));
                }
            }
        }
    }
    let mut cells: Vec<_> = cells.into_iter().collect();
    cells.sort_by(|a, b| {
        let distance = |(x, y): (i32, i32)| {
            ((x as f32 + 0.5) * 0.5 - DENSITY_CENTER[0])
                .hypot((y as f32 + 0.5) * 0.5 - DENSITY_CENTER[1])
        };
        distance(a.0).total_cmp(&distance(b.0)).then(a.0.cmp(&b.0))
    });
    let first_id = program
        .statements
        .iter()
        .filter_map(|s| match s {
            Statement::Call(c) => c.assigns.first(),
            _ => None,
        })
        .filter_map(|n| {
            n.strip_prefix("generated_area_interior_")?
                .parse::<usize>()
                .ok()
        })
        .max()
        .map_or(0, |id| id + 1);
    let mut count = 0;
    let mut source = String::new();
    'cells: for ((x, y), samples) in cells {
        for lot in 0..4 {
            if count >= budget {
                break 'cells;
            }
            let p = [
                (x as f32 + 0.25 + (lot % 2) as f32 * 0.5) * 0.5,
                (y as f32 + 0.25 + (lot / 2) as f32 * 0.5) * 0.5,
            ];
            let distance = |q: [f32; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
            let Some(&(road, u)) = samples
                .iter()
                .min_by(|a, b| distance(a.0).total_cmp(&distance(b.0)))
            else {
                continue;
            };
            if distance(road) > 1.8 {
                continue;
            }
            let v = [-u[1], u[0]];
            for (w, d) in [(0.35, 0.35), (0.27, 0.27), (0.22, 0.22), (0.16, 0.20)] {
                let q = [
                    [-w / 2., -d / 2.],
                    [w / 2., -d / 2.],
                    [w / 2., d / 2.],
                    [-w / 2., d / 2.],
                ]
                .map(|a| {
                    [
                        p[0] + a[0] * u[0] + a[1] * v[0],
                        p[1] + a[0] * u[1] + a[1] * v[1],
                    ]
                })
                .to_vec();
                if index.overlaps(&q, None, false) || green.overlaps(&q, None, true) {
                    continue;
                }
                let Some((low, high)) = supported(&q, terrain) else {
                    continue;
                };
                let central = (36. ..52.).contains(&p[0]) && (-3. ..20.).contains(&p[1]);
                let height = if central {
                    0.10 + (count % 4) as f32 * 0.025
                } else {
                    0.075 + (count % 3) as f32 * 0.025
                };
                let material = if count % 4 == 0 { "concrete" } else { "brick" };
                let xy = q
                    .iter()
                    .map(|p| format!("{:.7},{:.7}", p[0], p[1]))
                    .collect::<Vec<_>>()
                    .join(",");
                let id = first_id + count;
                let name = format!("generated_area_interior_{id}");
                writeln!(
                    source,
                    "{}=building({:.7},{{use:residential,material:{}}},{});",
                    name,
                    height + high - low,
                    material,
                    xy
                )?;
                writeln!(source, "elevation({name},{:.7});", low - 0.002)?;
                index.insert(q);
                count += 1;
                break;
            }
        }
    }
    eprintln!("Added {count} block-interior buildings");
    Ok((opencity::parse(&source)?.statements, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interior_fill_obeys_its_budget_and_keeps_roads_and_green_space_clear() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1024, 1024);
            encoder.set_color(png::ColorType::Grayscale);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![128; 1024 * 1024])
                .unwrap();
        }
        let terrain = ElevationMap::from_png(
            opencity::heightmap::ElevationMapSpec {
                path: String::new(),
                origin: [-2., -2.],
                size: [4., 4.],
                heights: [0.1, 0.1],
                water_level: None,
            },
            &bytes,
        )
        .unwrap();
        let program = opencity::parse(
            "road sd_street: sidewalk carlane carlane sidewalk; end
            a=sd_street(0,0,1,0); elevation(a,0.1);",
        )
        .unwrap();
        let mut index = Index::default();
        let mut green = Index::default();
        index.insert(vec![[-2., -0.102], [2., -0.102], [2., 0.102], [-2., 0.102]]);
        green.insert(vec![[0.3, -2.], [2., -2.], [2., 2.], [0.3, 2.]]);
        let (statements, count) = build(&program, &terrain, &mut index, &green, 5).unwrap();
        assert_eq!(count, 5);
        let features = opencity::project_landscape_features(&Program { statements });
        assert!(features.errors.is_empty());
        for f in features.features {
            assert!(!green.overlaps(&f.points, None, true));
            assert!(
                f.points.iter().all(|p| p[1] > 0.102) || f.points.iter().all(|p| p[1] < -0.102)
            );
        }
    }
}
