//! Static broad phase: only nearby building footprints reach vehicle contact geometry.
use super::METERS_PER_UNIT;
use glam::{Vec2, Vec3};
use opencity::{runtime::City, LandscapeKind};
use std::collections::HashMap;
const CELL: f32 = 50.;
pub(super) struct Buildings {
    cells: HashMap<(i32, i32), Vec<usize>>,
}
impl Buildings {
    /// Sweep a padded camera along the follow arm, pulling in immediately at
    /// the first obstruction. Sampling is finer than the clearance radius so
    /// thin walls cannot be skipped. The caller eases the return to full range.
    pub fn camera_eye(&self, city: &City, anchor: Vec3, desired: Vec3) -> Vec3 {
        self.clip_segment(city, anchor, desired, 0.6)
    }
    pub(super) fn clip_segment(
        &self,
        city: &City,
        anchor: Vec3,
        desired: Vec3,
        radius: f32,
    ) -> Vec3 {
        let anchor = anchor * METERS_PER_UNIT;
        let desired = desired * METERS_PER_UNIT;
        let delta = desired - anchor;
        let mut candidates = Vec::new();
        self.nearby(
            (anchor + desired).truncate() * 0.5,
            delta.truncate().length() * 0.5 + radius,
            &mut candidates,
        );
        if candidates.is_empty() {
            return desired / METERS_PER_UNIT;
        }
        let blocked = |p: Vec3| {
            candidates.iter().any(|&id| {
                if let Some(shape) = city.model_colliders.get(&id) {
                    if shape
                        .contact(
                            (p.truncate() / METERS_PER_UNIT).to_array(),
                            radius / METERS_PER_UNIT,
                            (p.z - radius) / METERS_PER_UNIT,
                            (p.z + radius) / METERS_PER_UNIT,
                        )
                        .is_some()
                    {
                        return true;
                    }
                    if !city.foundation_overlaps_height(
                        id,
                        (p.z - radius) / METERS_PER_UNIT,
                        (p.z + radius) / METERS_PER_UNIT,
                    ) {
                        return false;
                    }
                }
                let building = &city.landscape[id];
                if city.building_bottom(id) * METERS_PER_UNIT >= p.z + radius
                    || (building.elevation + building.height.unwrap_or(0.)) * METERS_PER_UNIT
                        <= p.z - radius
                {
                    return false;
                }
                let point = p.truncate();
                let mut inside = false;
                let mut near_edge = false;
                for (a, b) in building
                    .points
                    .iter()
                    .zip(building.points.iter().cycle().skip(1))
                {
                    let a = Vec2::from_array(*a) * METERS_PER_UNIT;
                    let b = Vec2::from_array(*b) * METERS_PER_UNIT;
                    let edge = b - a;
                    if (a.y > point.y) != (b.y > point.y)
                        && point.x < edge.x * (point.y - a.y) / edge.y + a.x
                    {
                        inside = !inside;
                    }
                    let along =
                        ((point - a).dot(edge) / edge.length_squared().max(1e-10)).clamp(0., 1.);
                    near_edge |= point.distance_squared(a + edge * along) <= radius * radius;
                }
                inside || near_edge
            })
        };
        let steps = (delta.length() / 0.2).ceil().max(1.) as usize;
        let mut safe = 0.;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            if blocked(anchor + delta * t) {
                let mut hit = t;
                for _ in 0..8 {
                    let middle = (safe + hit) * 0.5;
                    if blocked(anchor + delta * middle) {
                        hit = middle;
                    } else {
                        safe = middle;
                    }
                }
                return (anchor + delta * safe) / METERS_PER_UNIT;
            }
            safe = t;
        }
        desired / METERS_PER_UNIT
    }
    /// Intersect vertical footprint faces in metres, retaining the actual face normal.
    pub(super) fn ray_face(
        &self,
        city: &City,
        from: Vec3,
        dir: Vec3,
        limit: f32,
    ) -> Option<(f32, Vec3, super::ShotSurface)> {
        let mut candidates = Vec::new();
        self.nearby(
            (from + dir * limit * 0.5).truncate(),
            dir.truncate().length() * limit * 0.5,
            &mut candidates,
        );
        let mut nearest = limit;
        let mut hit = None;
        for id in candidates {
            let b = &city.landscape[id];
            let low = city.building_bottom(id) * METERS_PER_UNIT;
            let base = b.elevation * METERS_PER_UNIT;
            let high = if let Some(shape) = city.model_colliders.get(&id) {
                if let Some((t, normal)) = shape.ray(
                    (from / METERS_PER_UNIT).to_array(),
                    (dir / METERS_PER_UNIT).to_array(),
                    nearest,
                ) {
                    nearest = t;
                    hit = Some((
                        t,
                        Vec3::from(normal),
                        super::ShotSurface::facade(b.building_style.unwrap_or_default().material),
                    ));
                }
                if low >= base {
                    continue;
                }
                base
            } else {
                base + b.height.unwrap_or(0.) * METERS_PER_UNIT
            };
            for (a, end) in b.points.iter().zip(b.points.iter().cycle().skip(1)) {
                let edge = (Vec2::from(*end) - Vec2::from(*a)) * METERS_PER_UNIT;
                let offset = Vec2::from(*a) * METERS_PER_UNIT - from.truncate();
                let denom = dir.truncate().perp_dot(edge);
                if denom.abs() < 1e-6 {
                    continue;
                }
                let t = offset.perp_dot(edge) / denom;
                let u = offset.perp_dot(dir.truncate()) / denom;
                let z = from.z + dir.z * t;
                if t >= 0. && t <= nearest && (0.0..=1.0).contains(&u) && (low..=high).contains(&z)
                {
                    let n = Vec3::new(-edge.y, edge.x, 0.) / edge.length();
                    nearest = t;
                    hit = Some((
                        t,
                        if n.dot(dir) > 0. { -n } else { n },
                        if z < base {
                            super::ShotSurface::Concrete
                        } else {
                            super::ShotSurface::facade(
                                b.building_style.unwrap_or_default().material,
                            )
                        },
                    ));
                }
            }
            if dir.z.abs() > 1e-6 {
                for (z, n) in [(high, Vec3::Z), (low, -Vec3::Z)] {
                    let t = (z - from.z) / dir.z;
                    if t < 0. || t > nearest || n.dot(dir) >= 0. {
                        continue;
                    }
                    let q = (from + dir * t).truncate() / METERS_PER_UNIT;
                    let mut inside = false;
                    for (a, b) in b.points.iter().zip(b.points.iter().cycle().skip(1)) {
                        if (a[1] > q.y) != (b[1] > q.y)
                            && q.x < (b[0] - a[0]) * (q.y - a[1]) / (b[1] - a[1]) + a[0]
                        {
                            inside = !inside;
                        }
                    }
                    if inside {
                        nearest = t;
                        hit = Some((t, n, super::ShotSurface::Concrete));
                    }
                }
            }
        }
        hit
    }
    pub fn new(city: &City) -> Self {
        let mut cells = HashMap::<_, Vec<_>>::new();
        for (id, feature) in city.landscape.iter().enumerate() {
            if feature.kind != LandscapeKind::Building
                || !feature.building_style.unwrap_or_default().collidable
            {
                continue;
            }
            let (min, max) = feature.points.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(min, max), point| {
                    let p = Vec2::from_array(*point) * METERS_PER_UNIT;
                    (min.min(p), max.max(p))
                },
            );
            for x in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
                for y in (min.y / CELL).floor() as i32..=(max.y / CELL).floor() as i32 {
                    cells.entry((x, y)).or_default().push(id);
                }
            }
        }
        Self { cells }
    }
    pub fn nearby(&self, center: Vec2, reach: f32, output: &mut Vec<usize>) {
        output.clear();
        for x in
            ((center.x - reach) / CELL).floor() as i32..=((center.x + reach) / CELL).floor() as i32
        {
            for y in ((center.y - reach) / CELL).floor() as i32
                ..=((center.y + reach) / CELL).floor() as i32
            {
                if let Some(ids) = self.cells.get(&(x, y)) {
                    output.extend_from_slice(ids);
                }
            }
        }
        output.sort_unstable();
        output.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn julep_globe_and_stand_have_separate_contact_and_ray_bounds() {
        let map = crate::hermes::Hermes::test_map_from_source(concat!(
            "site=building(0.246,\"study_01\",-1.157,-1.323,-1.157,-1.077,-1.403,-1.077,-1.403,-1.323);",
            "elevation(site,0.102);"
        ));
        let city = map.city();
        let buildings = Buildings::new(city);
        // Map X increases toward the counter, map Y along its front.
        let stand = [-1.166, -1.15];
        let corner = [-1.166, -1.30];
        let globe = [-1.288, -1.20];
        assert!(city.building_collision(stand, 0.001, 0.115, 0.13));
        assert!(!city.building_collision(stand, 0.001, 0.17, 0.18));
        assert!(!city.building_collision(corner, 0.001, 0.115, 0.13));
        assert!(city.building_collision(globe, 0.001, 0.32, 0.33));
        let roof = |xy: [f32; 2]| {
            buildings
                .ray_face(
                    city,
                    Vec2::from(xy).extend(0.4) * METERS_PER_UNIT,
                    -Vec3::Z,
                    0.4 * METERS_PER_UNIT,
                )
                .map(|(t, _, _)| 0.4 - t / METERS_PER_UNIT)
        };
        assert!((roof(stand).unwrap() - 0.1475).abs() < 0.001);
        assert!((roof(globe).unwrap() - 0.348).abs() < 0.001);
        assert!(roof(corner).is_none());
        let eye = Vec2::from(corner).extend(0.12);
        assert!(
            buildings
                .camera_eye(city, eye + Vec3::X * 0.03, eye)
                .distance(eye)
                < 1e-5
        );
    }

    #[test]
    fn report_91_foundation_closes_downhill_wall_and_stops_camera() {
        let source = include_str!("../../data/maps/mtl/map.road")
            .lines()
            .filter(|line| {
                line.starts_with("elevation_map(")
                    || line.starts_with("placed_building_13 =")
                    || line.starts_with("elevation(placed_building_13,")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let city =
            City::from_program_with_elevation_loader(&opencity::parse(&source).unwrap(), |spec| {
                opencity::heightmap::ElevationMap::from_png(
                    spec.clone(),
                    include_bytes!("../../data/maps/mtl/elevation.png"),
                )
            })
            .unwrap();
        let feature = &city.landscape[0];
        let x = 1.3684925;
        let y = feature.points[0][1];
        let ground = city.surface_at([x, y], f32::INFINITY).unwrap().height;
        let base = feature.elevation;
        assert!(
            base - ground > 0.01,
            "reported downhill gap {ground}..{base}"
        );
        let mesh = city
            .mesh(
                opencity::runtime::Region::Radius {
                    center: [x, y],
                    radius: 0.01,
                },
                0.15,
            )
            .unwrap();
        let wall = mesh
            .vertices
            .chunks_exact(3)
            .filter(|face| face[0].surface[3] == -2. && face.iter().all(|v| v.position[1] == y))
            .flatten()
            .map(|v| v.position[2])
            .collect::<Vec<_>>();
        assert!(wall.iter().any(|&z| z <= ground) && wall.contains(&base));

        let middle = (ground + base) * 0.5;
        assert!(city.building_collision([x, y - 0.001], 0., middle, base));
        let anchor = Vec3::new(x, y + 0.03, middle);
        let inside = Vec3::new(x, y - 0.02, middle);
        let buildings = Buildings::new(&city);
        assert!(buildings.camera_eye(&city, anchor, inside).y > y);
    }

    #[test]
    fn camera_stops_before_thin_wall_even_with_clear_endpoint_and_handles_roofs() {
        let city = City::from_program(
            &opencity::parse("wall=building(0.1,-0.05,-0.1,-0.049,-0.1,-0.049,0.1,-0.05,0.1);")
                .unwrap(),
            None,
        )
        .unwrap();
        let buildings = Buildings::new(&city);
        let anchor = Vec3::new(0., 0., 0.02);
        let desired = Vec3::new(-0.15, 0., 0.04);
        let eye = buildings.camera_eye(&city, anchor, desired);
        assert!(eye.x > -0.0431 && eye.x < -0.04, "{eye:?}");
        assert_eq!(
            buildings.camera_eye(&city, anchor + Vec3::Z, desired + Vec3::Z),
            desired + Vec3::Z
        );
        assert_eq!(
            buildings.camera_eye(&city, anchor, anchor + Vec3::X),
            anchor + Vec3::X
        );
        for angle in -90..=90 {
            let theta = (angle as f32).to_radians();
            let target = anchor + Vec3::new(-theta.cos() * 0.15, theta.sin() * 0.15, 0.02);
            let eye = buildings.camera_eye(&city, anchor, target);
            assert!(!city.building_collision(
                eye.truncate().to_array(),
                0.0059,
                eye.z - 0.0059,
                eye.z + 0.0059
            ));
        }
    }
}
