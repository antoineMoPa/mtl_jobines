//! Habitat queries take world metres. Authored polygons and the elevation mesh use map units.
use super::{Species, METERS_PER_UNIT};
use glam::{Vec2, Vec3};
use opencity::{
    heightmap::{ElevationMap, ELEVATION_MAP_SIZE},
    runtime::{City, SurfaceKind},
    Argument, BuildingUse, Call, LandscapeFeature, LandscapeKind, Program, Statement,
};
use rapier3d::{
    na::Point3,
    parry::{query::PointQuery, shape::Polyline},
};

#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct Habitat {
    pub(crate) island: bool,
    pub(crate) residential: bool,
    pub(crate) downtown: bool,
    secondary_pigeon: bool,
    pub(crate) parking: bool,
    pub(crate) airport_grass: bool,
    pub(crate) shore: bool,
    wading_depth: bool,
}

impl Habitat {
    pub(crate) fn permits(&self, species: Species) -> bool {
        match species {
            Species::Cat => self.residential,
            Species::Fox => self.airport_grass,
            Species::Gull => self.parking,
            Species::Pigeon => self.downtown || self.secondary_pigeon,
            Species::Heron => self.shore && self.wading_depth,
            Species::Goose | Species::Mallard => self.shore,
            Species::Squirrel | Species::Chickadee | Species::Skunk | Species::Crow => self.island,
        }
    }
}

struct Polygon {
    points: Vec<Vec2>,
    min: Vec2,
    max: Vec2,
}

impl Polygon {
    fn from_feature(f: &LandscapeFeature) -> Self {
        let points: Vec<_> = f.points.iter().copied().map(Vec2::from_array).collect();
        Self {
            min: points.iter().copied().fold(Vec2::INFINITY, Vec2::min),
            max: points.iter().copied().fold(Vec2::NEG_INFINITY, Vec2::max),
            points,
        }
    }

    fn edges(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        self.points
            .iter()
            .copied()
            .zip(self.points.iter().copied().cycle().skip(1))
    }

    fn contains(&self, p: Vec2) -> bool {
        self.min.cmple(p).all()
            && self.max.cmpge(p).all()
            && self.edges().fold(false, |inside, (a, b)| {
                inside
                    ^ ((a.y > p.y) != (b.y > p.y)
                        && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y))
            })
    }

    fn edge_distance(&self, p: Vec2) -> f32 {
        self.edges()
            .map(|(a, b)| {
                let edge = b - a;
                let t = ((p - a).dot(edge) / edge.length_squared().max(f32::MIN_POSITIVE))
                    .clamp(0., 1.);
                p.distance_squared(a + edge * t)
            })
            .fold(f32::INFINITY, f32::min)
            .sqrt()
    }

    fn within(&self, p: Vec2, margin: f32) -> bool {
        p.distance_squared(p.clamp(self.min, self.max)) <= margin * margin
            && (self.contains(p) || self.edge_distance(p) <= margin)
    }
}

#[derive(Default)]
pub(crate) struct Habitats {
    islands: Vec<Polygon>,
    reclaimed: Vec<Polygon>,
    downtown: Vec<Polygon>,
    parking: Vec<Polygon>,
    airport: Vec<Polygon>,
    residential_edges: Option<Polyline>,
    waterline: Option<Polyline>,
}

impl Habitats {
    pub(crate) fn read(program: &Program, city: &City) -> Self {
        let mut result = Self::default();
        // Polyline builds a spatial index over the placed footprints. Sample the
        // nearest facade rather than scanning every house at each wildlife probe.
        let mut vertices = Vec::new();
        let mut edges = Vec::new();
        for f in &city.landscape {
            if f.kind == LandscapeKind::Building && f.building_use == BuildingUse::Residential {
                let start = vertices.len() as u32;
                let count = f.points.len() as u32;
                for (i, &[x, y]) in f.points.iter().enumerate() {
                    vertices.push(Point3::new(x, y, 0.));
                    edges.push([start + i as u32, start + (i as u32 + 1) % count]);
                }
            }
            let shapes = match (f.name.as_deref(), f.kind) {
                (Some("sphere_parking"), LandscapeKind::Concrete) => &mut result.parking,
                (Some("airport_shore"), LandscapeKind::Land) => &mut result.airport,
                (Some("airport_bank"), LandscapeKind::Land) => &mut result.reclaimed,
                (Some(name), LandscapeKind::Building) if name.starts_with("district_downtown_") => {
                    &mut result.downtown
                }
                (Some(name), LandscapeKind::Land)
                    if name == "island"
                        || name.ends_with("_island")
                        || name.ends_with("_islet") =>
                {
                    &mut result.islands
                }
                _ => continue,
            };
            shapes.push(Polygon::from_feature(f));
        }
        if !edges.is_empty() {
            result.residential_edges = Some(Polyline::new(vertices, Some(edges)));
        }

        // Unplaced terrain_source is the editable recipe for the elevation PNG.
        // Project it with the same block placement/rotation logic as the terrain baker.
        let mut recipe = Program {
            statements: program
                .statements
                .iter()
                .filter(|s| matches!(s, Statement::Block(_)))
                .cloned()
                .collect(),
        };
        if recipe
            .statements
            .iter()
            .any(|s| matches!(s, Statement::Block(b) if b.name == "terrain_source"))
        {
            let mut placements = program
                .statements
                .iter()
                .filter(|s| matches!(s, Statement::Call(c) if c.name == "terrain_source"))
                .cloned()
                .peekable();
            if placements.peek().is_none() {
                recipe.statements.push(Statement::Call(Call {
                    name: "terrain_source".into(),
                    assigns: Vec::new(),
                    args: vec![Argument::Number("0".into()); 2],
                }));
            } else {
                recipe.statements.extend(placements);
            }
            for f in opencity::project_landscape_features(&recipe)
                .features
                .into_iter()
                .filter(|f| f.kind == LandscapeKind::Land)
            {
                let shapes = match f.name.as_deref() {
                    Some("airport_bank" | "airport_shore") => &mut result.reclaimed,
                    _ => &mut result.islands,
                };
                let polygon = Polygon::from_feature(&f);
                if !shapes.iter().any(|p| p.points == polygon.points) {
                    shapes.push(polygon);
                }
            }
        }
        if let Some(elevation) = &city.elevation {
            result.waterline = result.trace_waterline(elevation);
        }
        result
    }

    pub(crate) fn nearest_shore(&self, point: Vec2) -> Option<Vec2> {
        let line = self.waterline.as_ref()?;
        let p = line
            .project_local_point(&Point3::new(point.x, point.y, 0.), false)
            .point;
        Some(Vec2::new(p.x, p.y))
    }

    pub(crate) fn at(&self, city: &City, point: Vec2) -> Habitat {
        let p = point / METERS_PER_UNIT;
        let Some(sample) = city.surface_at(p.into(), f32::MAX) else {
            return Habitat::default();
        };
        let open = matches!(sample.kind, SurfaceKind::Grass | SurfaceKind::Sand);
        let paved = sample.kind == SurfaceKind::Sidewalk;
        let in_water = sample.kind == SurfaceKind::Water;
        let mut habitat = Habitat::default();
        if !(open || paved || in_water)
            || city.building_bounds_overlap(
                (p - 0.002).extend(f32::MIN).into(),
                (p + 0.002).extend(f32::MAX).into(),
            )
        {
            return habitat;
        }
        let airport = self.airport.iter().any(|poly| poly.contains(p));
        habitat.airport_grass =
            airport && sample.kind == SurfaceKind::Grass && sample.road.is_none();
        let parking_lot = self.parking.iter().any(|poly| poly.contains(p));
        habitat.parking = paved && sample.road.is_none() && parking_lot;
        habitat.downtown = !airport
            && !habitat.parking
            && (open || paved)
            && self
                .downtown
                .iter()
                .any(|poly| poly.within(p, 65. / METERS_PER_UNIT));
        let on_island = airport
            || self
                .islands
                .iter()
                .chain(&self.reclaimed)
                .any(|poly| poly.contains(p));
        habitat.island = open && sample.road.is_none() && on_island;
        habitat.secondary_pigeon = paved && on_island && !airport && !habitat.parking;
        habitat.residential = (paved
            || (sample.kind == SurfaceKind::Grass && sample.road.is_none()))
            && (self.islands.is_empty() || on_island)
            && !airport
            && !parking_lot
            && self.residential_edges.as_ref().is_some_and(|edges| {
                edges.distance_to_local_point(&Point3::new(p.x, p.y, 0.), false)
                    <= 18. / METERS_PER_UNIT
            });

        let Some(elevation) = &city.elevation else {
            return habitat;
        };
        let Some(level) = elevation.spec.water_level else {
            return habitat;
        };
        let relative_height = (sample.height - level) * METERS_PER_UNIT;
        if (open || in_water)
            && sample.road.is_none()
            && !airport
            && (-0.45..=1.0).contains(&relative_height)
            && elevation.cut_at(p.into()).is_none()
            && self
                .nearest_shore(point)
                .is_some_and(|q| point.distance_squared(q) <= 15. * 15.)
        {
            habitat.shore = true;
            habitat.wading_depth = relative_height >= -0.35;
        }
        habitat
    }

    pub(crate) fn permits(&self, city: &City, species: Species, point: Vec2) -> bool {
        self.at(city, point).permits(species)
    }

    // Follow the PNG triangles' intersection with the water plane. Flood only
    // the offshore component so inland basins and excavations cannot seed a coast.
    fn trace_waterline(&self, elevation: &ElevationMap) -> Option<Polyline> {
        let level = elevation.spec.water_level?;
        let first = self.islands.first()?;
        let min = self
            .islands
            .iter()
            .fold(first.min, |p, poly| p.min(poly.min));
        let max = self
            .islands
            .iter()
            .fold(first.max, |p, poly| p.max(poly.max));
        let origin = Vec2::from(elevation.spec.origin);
        let pixel = Vec2::from(elevation.spec.size) / (ELEVATION_MAP_SIZE - 1) as f32;
        let last = Vec2::splat((ELEVATION_MAP_SIZE - 1) as f32);
        let lo = (((min - origin) / pixel).floor() - 2.)
            .clamp(Vec2::ZERO, last)
            .as_uvec2();
        let hi = (((max - origin) / pixel).ceil() + 2.)
            .clamp(Vec2::ZERO, last)
            .as_uvec2();
        let w = (hi.x - lo.x + 1) as usize;
        let h = (hi.y - lo.y + 1) as usize;
        let mut wet = vec![false; w * h];
        let mut ocean = vec![false; w * h];
        let mut stack = Vec::new();
        let vertex =
            |x: usize, y: usize| Vec3::from(elevation.point(lo.x as usize + x, lo.y as usize + y));
        for y in 0..h {
            for x in 0..w {
                let i = x + y * w;
                wet[i] = vertex(x, y).z < level;
                if wet[i] && (x == 0 || y == 0 || x + 1 == w || y + 1 == h) {
                    ocean[i] = true;
                    stack.push(i);
                }
            }
        }
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            // Each raster cell has the diagonal from its low to its high XY corner.
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1)] {
                let (xx, yy) = ((x + dx) as usize, (y + dy) as usize);
                if xx < w && yy < h {
                    let j = xx + yy * w;
                    if wet[j] && !ocean[j] {
                        ocean[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let mut vertices = Vec::new();
        let mut segments = Vec::new();
        for y in 0..h.saturating_sub(1) {
            for x in 0..w.saturating_sub(1) {
                let a = x + y * w;
                for ids in [[a, a + 1, a + w + 1], [a, a + w + 1, a + w]] {
                    if !ids.iter().any(|&i| ocean[i]) || ids.iter().all(|&i| wet[i]) {
                        continue;
                    }
                    let tri = ids.map(|i| vertex(i % w, i / w));
                    let mut crossings = [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])]
                        .into_iter()
                        .filter(|(p, q)| (p.z < level) != (q.z < level))
                        .map(|(p, q)| p.lerp(q, (level - p.z) / (q.z - p.z)).truncate());
                    let (Some(p), Some(q)) = (crossings.next(), crossings.next()) else {
                        continue;
                    };
                    if p.distance_squared(q) <= f32::EPSILON.powi(2) {
                        continue;
                    }
                    let center = (p + q) * 0.5;
                    // The runway platform can cover submerged PNG terrain. The
                    // separate airport bank can also share an interior island edge.
                    if self.airport.iter().any(|poly| poly.contains(center))
                        || !self
                            .islands
                            .iter()
                            .any(|poly| poly.within(center, pixel.length() * 2.))
                        || self
                            .reclaimed
                            .iter()
                            .any(|poly| poly.within(center, pixel.length() * 2.))
                    {
                        continue;
                    }
                    let id = vertices.len() as u32;
                    for p in [p, q] {
                        vertices.push(Point3::new(
                            p.x * METERS_PER_UNIT,
                            p.y * METERS_PER_UNIT,
                            0.,
                        ));
                    }
                    segments.push([id, id + 1]);
                }
            }
        }
        (!segments.is_empty()).then(|| Polyline::new(vertices, Some(segments)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_habitats_follow_rotated_blocks_and_require_clear_ground() {
        let program = opencity::parse(
            r#"
            island = land(0,0, 10,0, 10,10, 0,10);
            block attractions:
                airport_shore = land(0,0, 3,0, 3,1, 0,1);
                strip = concrete(0,0, 3,0, 3,0.2, 0,0.2);
                elevation(strip,0.01);
                sphere_parking = concrete(0,2, 1,2, 1,3, 0,3);
                elevation(sphere_parking,0.01);
                kiosk = building(0.6, { collision: none }, 0.4,2.4, 0.6,2.4, 0.6,2.6, 0.4,2.6);
                district_downtown_01 = building(0.4, 0,4, 1,4, 1,5, 0,5);
            end
            attractions(7,3,0,90);
            plaza = concrete(8,8, 9,8, 9,9, 8,9);
            elevation(plaza,0.01);
        "#,
        )
        .unwrap();
        let city = City::from_program(&program, None).unwrap();
        let habitats = Habitats::read(&program, &city);
        let at = |x: f32, y: f32| habitats.at(&city, Vec2::new(x, y) * METERS_PER_UNIT);
        assert!(at(6.4, 4.5).airport_grass);
        assert!(!at(6.9, 4.5).airport_grass, "runway");
        assert!(at(4.8, 3.2).parking);
        assert!(!at(4.5, 3.5).parking, "kiosk footprint");
        assert!(!at(4.8, 3.2).permits(Species::Pigeon));
        assert!(at(2.5, 4.2).downtown);
        assert!(!at(2.5, 3.5).downtown, "building");
        assert!(at(8.5, 8.5).permits(Species::Pigeon));
        assert!(!at(8.5, 8.5).downtown);
        assert!(at(1., 1.).island);
        assert!(!at(-1., 1.).island);
        assert!(habitats.nearest_shore(Vec2::ZERO).is_none());
    }

    #[test]
    fn cats_follow_placed_housing_edges_and_clear_residential_surfaces() {
        let program = opencity::parse(
            r#"
            site = land(0,0, 10,0, 10,10, 0,10);
            block row:
                flats = building(0.1, {use: residential}, 0,0, 4,0, 4,0.2, 0,0.2);
            end
            row(7,3,0,90);
            shop = building(0.1, {use: commercial}, 1,1, 1.2,1, 1.2,1.2, 1,1.2);
            mall = building(0.1, {use: mixed_use}, 1,3, 1.2,3, 1.2,3.2, 1,3.2);
            airport_shore = land(7,3.1, 7.18,3.1, 7.18,3.3, 7,3.3);
            sphere_parking = concrete(7,3.6, 7.18,3.6, 7.18,3.8, 7,3.8);
            elevation(sphere_parking,0.01);
            yard = concrete(7,4.1, 7.18,4.1, 7.18,4.3, 7,4.3);
            elevation(yard,0.01);
            road local: sidewalk carlane sidewalk; end
            street = local(7.11,5.3, 7.11,6.7);
            "#,
        )
        .unwrap();
        let city = City::from_program(&program, None).unwrap();
        let habitats = Habitats::read(&program, &city);
        let cats = |x, y| habitats.permits(&city, Species::Cat, Vec2::new(x, y) * METERS_PER_UNIT);
        assert!(
            cats(7.17, 4.7),
            "grass along the long wall of a rotated row"
        );
        assert!(cats(7.1, 4.2), "paved yard");
        assert!(!cats(7.19, 4.7), "beyond the 18 m residential belt");
        assert!(!cats(6.9, 4.6), "roof");
        assert!(!cats(1.3, 1.1), "shop");
        assert!(!cats(1.3, 3.1), "mixed-use building");
        assert!(!cats(7.1, 3.2), "airport");
        assert!(!cats(7.1, 3.7), "Julep lot");
        let mut sidewalk = false;
        let mut lane = false;
        for sample in 1..18 {
            let x = 7. + sample as f32 * 0.01;
            match city.surface_at([x, 6.], f32::MAX).unwrap().kind {
                SurfaceKind::Sidewalk => {
                    assert!(cats(x, 6.));
                    sidewalk = true;
                }
                SurfaceKind::Asphalt => {
                    assert!(!cats(x, 6.));
                    lane = true;
                }
                _ => {}
            }
        }
        assert!(sidewalk && lane);
    }

    #[test]
    fn waterline_tracks_the_bed_and_skips_inland_water_and_reclamation() {
        let program = opencity::parse(
            r#"
            elevation_map("bed.png",0,0,10.23,10.23,-0.1,0.1);
            water_level(0);
            block terrain_source:
                island = land(2,2, 8,2, 8,8, 2,8);
                airport_bank = land(1,4, 3,4, 3,6, 1,6);
            end
            airport_shore = land(1,4, 2.5,4, 2.5,6, 1,6);
            elevation(airport_shore,0.12);
            pond_home = building(0.1,{use:residential},5.42,4.9,5.55,4.9,5.55,5.1,5.42,5.1);
        "#,
        )
        .unwrap();
        // PNG pixels are one metre wide. The graded bank crosses the water plane at
        // the island perimeter. A second water body is enclosed in its interior.
        let mut pixels = Vec::with_capacity(1024 * 1024);
        for y in 0i32..1024 {
            for x in 0i32..1024 {
                let inland = (x - 200).min(800 - x).min(y - 200).min(800 - y);
                let pond = (x - 500).abs().max((y - 500).abs()) - 40;
                pixels.push((128 + inland.min(pond)).clamp(0, 255) as u8);
            }
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1024, 1024);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
        let city = City::from_program_with_elevation_loader(&program, |s| {
            ElevationMap::from_png(s.clone(), &bytes)
        })
        .unwrap();
        let habitats = Habitats::read(&program, &city);
        let shore = |p| habitats.permits(&city, Species::Mallard, p);
        let q = habitats.nearest_shore(Vec2::new(799., 510.)).unwrap();
        assert!((q.x - 800.5).abs() < 0.05);
        assert!(shore(q));
        assert!(shore(q + Vec2::X * 3.), "shallows");
        assert!(shore(q - Vec2::X * 4.), "bank");
        assert!(!shore(q + Vec2::X * 12.), "deep water");
        assert!(!shore(Vec2::new(540., 500.)), "inland pond");
        assert!(
            !habitats.permits(&city, Species::Cat, Vec2::new(535., 500.)),
            "pond beside house"
        );
        assert!(
            habitats.permits(&city, Species::Cat, Vec2::new(558., 500.)),
            "yard beside pond"
        );
        assert!(
            habitats.at(&city, Vec2::new(180., 500.)).island,
            "airfield is connected land"
        );
        assert!(habitats.at(&city, Vec2::new(180., 500.)).airport_grass);
        assert!(
            !shore(Vec2::new(200., 500.)),
            "submerged terrain below airport"
        );
        assert!(
            habitats
                .nearest_shore(Vec2::new(200., 500.))
                .unwrap()
                .distance(Vec2::new(200., 500.))
                > 90.
        );
    }
}
