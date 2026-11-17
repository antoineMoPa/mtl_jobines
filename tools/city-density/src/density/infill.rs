//! Continuous street-facing apartment rows in gaps left by the source-cell fill.
use super::{plan::Plan, supported, Index};
use opencity::{heightmap::ElevationMap, Program, Statement};
use std::{error::Error, fmt::Write};

pub(super) fn build(
    program: &Program,
    terrain: &ElevationMap,
    index: &mut Index,
    green: &Index,
    plan: Plan,
    budget: usize,
) -> Result<(Vec<Statement>, usize), Box<dyn Error>> {
    let projection = opencity::map::project(program);
    let mut streets: Vec<_> = projection
        .segments
        .iter()
        .filter(|r| plan.street(&r.road_name))
        .collect();
    // Central streets are filled first if the conservative scene budget is reached.
    streets.sort_by(|a, b| {
        let pa = a.point_at(0.5);
        let pb = b.point_at(0.5);
        (pa.x - plan.center()[0])
            .hypot(pa.y - plan.center()[1])
            .total_cmp(&(pb.x - plan.center()[0]).hypot(pb.y - plan.center()[1]))
            .then_with(|| a.name.cmp(&b.name))
    });
    let widths: std::collections::BTreeMap<_, _> = program
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
    let mut source = String::new();
    let mut accepted = 0;
    let mut attempts = 0;
    let mut building_road_rejects = 0;
    let mut green_rejects = 0;
    let mut terrain_rejects = 0;
    eprintln!("Infill along {} street pieces", streets.len());
    let first_id = program
        .statements
        .iter()
        .filter_map(|s| match s {
            Statement::Call(c) => c.assigns.first(),
            _ => None,
        })
        .filter_map(|n| {
            n.strip_prefix("generated_area_infill_")?
                .parse::<usize>()
                .ok()
        })
        .max()
        .map_or(0, |id| id + 1);
    for r in streets {
        let length = [r.start, r.controls[0], r.controls[1], r.end]
            .windows(2)
            .map(|v| (v[1].x - v[0].x).hypot(v[1].y - v[0].y))
            .sum::<f32>();
        let samples = (length / 0.02).ceil().max(2.) as usize;
        let mut arc = Vec::with_capacity(samples + 1);
        arc.push(0.);
        for i in 1..=samples {
            let a = r.point_at((i - 1) as f32 / samples as f32);
            let b = r.point_at(i as f32 / samples as f32);
            arc.push(arc[i - 1] + (b.x - a.x).hypot(b.y - a.y));
        }
        let total = arc[samples];
        if total < 0.001 {
            continue;
        }
        let slots = (total / 0.3).ceil().max(1.) as usize;
        let pitch = total / slots as f32;
        let frontage = pitch.clamp(0.18, 0.4) - 0.001;
        for slot in 0..slots {
            let distance = (slot as f32 + 0.5) * pitch;
            let i = arc.partition_point(|&d| d < distance).clamp(1, samples);
            let t = ((i - 1) as f32 + (distance - arc[i - 1]) / (arc[i] - arc[i - 1]).max(1e-7))
                / samples as f32;
            let center = r.point_at(t);
            let a = r.point_at((t - 0.001).max(0.));
            let b = r.point_at((t + 0.001).min(1.));
            let length = (b.x - a.x).hypot(b.y - a.y);
            if length < 1e-7 {
                continue;
            }
            let u = [(b.x - a.x) / length, (b.y - a.y) / length];
            if (r.height_at(t)
                - terrain
                    .surface_height_at([center.x, center.y])
                    .unwrap_or(0.))
            .abs()
                > 0.05
            {
                continue;
            }
            for side in [-1., 1.] {
                if accepted >= budget {
                    break;
                }
                let v = [-u[1] * side, u[0] * side];
                'position: for setback in [0.027, 0.045, 0.075] {
                    for (width, depth) in [
                        (frontage, 0.38),
                        (frontage, 0.28),
                        (frontage, 0.20),
                        (0.19, 0.24),
                    ] {
                        let offset = widths[&r.road_name] / 2. + setback + depth / 2.;
                        let p = [center.x + v[0] * offset, center.y + v[1] * offset];
                        let q = [
                            [-width / 2., -depth / 2.],
                            [width / 2., -depth / 2.],
                            [width / 2., depth / 2.],
                            [-width / 2., depth / 2.],
                        ]
                        .map(|d| {
                            [
                                p[0] + u[0] * d[0] + v[0] * d[1],
                                p[1] + u[1] * d[0] + v[1] * d[1],
                            ]
                        })
                        .to_vec();
                        if !q.iter().all(|&p| plan.contains(p)) {
                            continue;
                        }
                        attempts += 1;
                        if index.overlaps(&q, None, false) {
                            building_road_rejects += 1;
                            continue;
                        }
                        if green.overlaps(&q, None, true) {
                            green_rejects += 1;
                            continue;
                        }
                        let Some((low, high)) = supported(&q, terrain) else {
                            terrain_rejects += 1;
                            continue;
                        };
                        let (height, style) = plan.building(p, accepted, true);
                        let xy = q
                            .iter()
                            .map(|p| format!("{:.7},{:.7}", p[0], p[1]))
                            .collect::<Vec<_>>()
                            .join(",");
                        let id = first_id + accepted;
                        let name = format!("generated_area_infill_{id}");
                        writeln!(
                            source,
                            "{}=building({:.7},{{{}}},{});",
                            name,
                            height + high - low,
                            style,
                            xy
                        )?;
                        writeln!(source, "elevation({name},{:.7});", low - 0.002)?;
                        index.insert(q);
                        accepted += 1;
                        break 'position;
                    }
                }
            }
        }
    }
    eprintln!("Infill {accepted}; attempts {attempts}");
    eprintln!("Rejected: roads/buildings {building_road_rejects}, green {green_rejects}");
    eprintln!("Rejected: terrain {terrain_rejects}");
    Ok((opencity::parse(&source)?.statements, accepted))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_street_pieces_receive_frontage_and_respect_green_space() {
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
                origin: [-1., -1.],
                size: [2., 2.],
                heights: [0.1, 0.1],
                water_level: None,
                cuts: Vec::new(),
            },
            &bytes,
        )
        .unwrap();
        let program = opencity::parse(
            "road sd_street: sidewalk carlane carlane sidewalk; end
            a=sd_street(0,0,0.08,0,0.16,0,0.24,0); elevation(a,0.1);",
        )
        .unwrap();
        let mut green = Index::default();
        green.insert(vec![[-1., 0.1], [1., 0.1], [1., 1.], [-1., 1.]]);
        let (statements, count) = build(
            &program,
            &terrain,
            &mut Index::default(),
            &green,
            Plan::SourceCity,
            10,
        )
        .unwrap();
        assert_eq!(
            count, 1,
            "the street is shorter than one usual frontage pitch"
        );
        let features = opencity::project_landscape_features(&Program { statements });
        assert!(features.errors.is_empty());
        assert!(features.features[0].points.iter().all(|p| p[1] < -0.1));
    }
}
