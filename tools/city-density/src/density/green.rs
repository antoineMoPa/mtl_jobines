//! Anonymous protected land polygons from local source tags, including relation outers.
use super::{Index, Polygon};
use osmpbfreader::{NodeId, OsmId, OsmObj, OsmPbfReader, Tags};
use serde_json::Value;
use std::{collections::HashSet, error::Error, fmt::Write, fs};

fn protected(tags: &Tags) -> bool {
    tags.get("leisure").is_some_and(|v| {
        matches!(
            v.as_str(),
            "park" | "garden" | "nature_reserve" | "golf_course"
        )
    }) || tags.get("landuse").is_some_and(|v| {
        matches!(
            v.as_str(),
            "forest" | "cemetery" | "recreation_ground" | "allotments"
        )
    }) || tags
        .get("natural")
        .is_some_and(|v| matches!(v.as_str(), "wood" | "wetland"))
}
fn point(p: [f64; 2], calibration: &Value) -> [f32; 2] {
    let a = &calibration["fit_controls"][0]["lonlat"];
    let b = &calibration["fit_controls"][1]["lonlat"];
    let c = &calibration["fit_controls"][2]["lonlat"];
    let merc = |lat: f64| {
        (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.)
            .tan()
            .ln()
    };
    let west = a[0].as_f64().unwrap();
    let east = b[0].as_f64().unwrap();
    let north = merc(a[1].as_f64().unwrap());
    let south = merc(c[1].as_f64().unwrap());
    [
        (-76.8 + (p[0] - west) * 153.6 / (east - west)) as f32,
        (-76.8 + (merc(p[1]) - north) * 153.6 / (south - north)) as f32,
    ]
}
pub(super) fn extract(base: &str) -> Result<Index, Box<dyn Error>> {
    let cached = format!("{base}/green-spaces.road");
    let source = fs::read_to_string(&cached)?;
    let features = opencity::project_landscape_features(&opencity::parse(&source)?);
    if !features.errors.is_empty() {
        return Err("invalid green-space geometry".into());
    }
    let mut index = Index::default();
    for f in features.features {
        if f.points.len() < 3 {
            return Err("green-space polygon needs three points".into());
        }
        index.insert(f.points);
    }
    Ok(index)
}
pub(crate) fn export(pbf: &str, calibration: &str, output: &str) -> Result<(), Box<dyn Error>> {
    let calibration: Value = serde_json::from_slice(&fs::read(calibration)?)?;
    let mut reader = OsmPbfReader::new(fs::File::open(pbf)?);
    let objects =
        reader.get_objs_and_deps(|o| (o.is_way() || o.is_relation()) && protected(o.tags()))?;
    let mut index = Index::default();
    let mut source = String::from(concat!(
        "# Protected green-space outers; anonymous local source geometry.\n",
        "# Authoring reservations only: terrain already supplies the grass surface.\n",
        "# Source attribution and license: see data/maps/mtl/README.md.\n"
    ));
    let mut members = HashSet::new();
    let mut rings = Vec::<Vec<NodeId>>::new();
    for relation in objects
        .values()
        .filter_map(OsmObj::relation)
        .filter(|r| protected(&r.tags))
    {
        let mut pieces: Vec<_> = relation
            .refs
            .iter()
            .filter(|r| r.role == "outer" || r.role.is_empty())
            .filter_map(|r| objects.get(&r.member).and_then(OsmObj::way))
            .map(|w| {
                members.insert(w.id);
                w.nodes.clone()
            })
            .collect();
        while let Some(mut ring) = pieces.pop() {
            while ring.first() != ring.last() {
                let Some(i) = pieces
                    .iter()
                    .position(|p| p.first() == ring.last() || p.last() == ring.last())
                else {
                    break;
                };
                let mut next = pieces.swap_remove(i);
                if next.last() == ring.last() {
                    next.reverse();
                }
                ring.extend(next.into_iter().skip(1));
            }
            if ring.first() == ring.last() {
                rings.push(ring);
            }
        }
    }
    for way in objects
        .values()
        .filter_map(OsmObj::way)
        .filter(|w| protected(&w.tags) && !members.contains(&w.id))
    {
        if way.nodes.first() == way.nodes.last() {
            rings.push(way.nodes.clone());
        }
    }
    for ring in rings {
        let q: Polygon = ring
            .iter()
            .map(|id| {
                let n = objects
                    .get(&OsmId::Node(*id))
                    .and_then(OsmObj::node)
                    .ok_or("missing green-space node")?;
                Ok(point([n.lon(), n.lat()], &calibration))
            })
            .collect::<Result<_, Box<dyn Error>>>()?;
        if q.len() < 4 || !q.iter().any(|p| p[0].abs() < 76.8 && p[1].abs() < 76.8) {
            continue;
        }
        let xy = q[..q.len() - 1]
            .iter()
            .map(|p| format!("{:.7},{:.7}", p[0], p[1]))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            source,
            "protected_green_{}=land({xy});",
            index.polygons.len()
        )?;
        index.insert(q);
    }
    fs::write(output, source)?;
    Ok(())
}
fn contains(q: &[[f32; 2]], p: [f32; 2]) -> bool {
    let mut inside = false;
    for (a, b) in q.iter().zip(q.iter().cycle().skip(1)) {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}
pub(super) fn overlap(a: &[[f32; 2]], b: &[[f32; 2]]) -> bool {
    if a.iter().any(|&p| contains(b, p)) || b.iter().any(|&p| contains(a, p)) {
        return true;
    }
    let cross = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    for (p, q) in a.iter().zip(a.iter().cycle().skip(1)) {
        for (r, s) in b.iter().zip(b.iter().cycle().skip(1)) {
            if cross(*p, *q, *r) * cross(*p, *q, *s) < 0.
                && cross(*r, *s, *p) * cross(*r, *s, *q) < 0.
            {
                return true;
            }
        }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concave_green_boundary_preserves_the_unprotected_notch() {
        let green = vec![[0., 0.], [4., 0.], [4., 1.], [1., 1.], [1., 4.], [0., 4.]];
        let notch = vec![[2., 2.], [3., 2.], [3., 3.], [2., 3.]];
        let crossing = vec![[0.5, 2.], [2., 2.], [2., 3.], [0.5, 3.]];
        assert!(!overlap(&green, &notch));
        assert!(overlap(&green, &crossing));
    }
}
