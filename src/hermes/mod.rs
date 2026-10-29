//! Static map/navigation access. OpenCity owns authored roads and geometry;
//! Hermes owns navigation, routing and the game’s map presentation.
mod building_style;
pub(crate) mod commerce_mesh;
mod commerce_props;
mod commercial;
mod game_data;
mod minimap;

mod routing;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data))]
use game_data::CacheKey;
use game_data::GameData;
pub use minimap::MinimapView;
pub(crate) use minimap::TEXTURE_SIZE;
use opencity::runtime::City;
use opencity::runtime::{Mesh, Region};
pub use routing::RouteStatus;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;
/// Both platforms expand the same source, differing only in elevation asset access.
fn expand(
    source: &opencity::Program,
    mut compile: impl FnMut(&opencity::Program) -> Result<City, String>,
) -> Result<(opencity::Program, City), String> {
    let mut program = opencity::runtime::scaffold::expand_program(source, &mut compile)?;
    dress_scaffolds(&mut program);
    let city = compile(&program)?;

    Ok((program, city))
}
/// Architectural palette belongs to the game; shared footprints, elevation and
/// height pass through unchanged to collision, preview and rendering.
fn dress_scaffolds(program: &mut opencity::Program) {
    use opencity::{Argument, Statement};
    for statement in &mut program.statements {
        let Statement::Call(call) = statement else {
            continue;
        };
        if call.name != "building" {
            continue;
        }
        let Some(name) = call.assigns.first() else {
            continue;
        };
        if !name.starts_with("generated_shop_") && !name.starts_with("generated_home_") {
            continue;
        }
        let seed = commercial::seed(name);
        let material = match (seed >> 16) % 10 {
            0..=4 => "red_brick",
            5..=6 => "brown_brick",
            7 => "buff_brick",
            8 => "limestone",
            _ => "pale_brick",
        };
        if let Some(Argument::Object(options)) = call.args.get_mut(1) {
            if let Some((_, value)) = options.iter_mut().find(|(key, _)| key == "material") {
                *value = Argument::Identifier(material.into());
            }
        }
    }
}
pub struct Hermes {
    city: Arc<City>,
    minimap: minimap::Minimap,
    prepared: Option<(Mesh, Mesh)>,
    router: routing::Router,
    navigation: routing::Navigation,
    #[cfg(not(target_arch = "wasm32"))]
    cache: Option<(CacheKey, std::path::PathBuf)>,
}
impl Hermes {
    #[cfg(all(not(target_arch = "wasm32"), test))]
    pub fn load(path: &Path) -> Result<Self, String> {
        Self::load_with_game_data(path, None)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_with_game_data(path: &Path, packaged: Option<&Path>) -> Result<Self, String> {
        let source = std::fs::read(path).map_err(|e| e.to_string())?;
        let parsed = opencity::parse(std::str::from_utf8(&source).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let elevation_spec = opencity::heightmap::ElevationMapSpec::from_program(&parsed)?;
        let elevation = elevation_spec
            .as_ref()
            .map(|spec| std::fs::read(path.parent().unwrap_or(Path::new(".")).join(&spec.path)))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        // All expansion stages retain the authored terrain. Decode it once,
        // sharing the source bytes with the cache fingerprint below.
        let elevation_map = elevation_spec
            .map(|spec| opencity::heightmap::ElevationMap::from_png(spec, &elevation))
            .transpose()?;
        let (program, city) = expand(&parsed, |p| {
            City::from_program_with_elevation_loader(p, |_| {
                elevation_map
                    .clone()
                    .ok_or_else(|| "Missing elevation asset".into())
            })
        })?;
        let key = CacheKey::for_map(&source, &elevation);
        let cache_path = game_data::cache_path(key);
        let mut data = None;
        if let Some(path) = packaged {
            match GameData::read(path, key) {
                Ok(value) => {
                    eprintln!("Using packaged game data: {}", path.display());
                    data = Some(value);
                }
                Err(error) => {
                    eprintln!("Packaged game data unavailable ({error}); preparing fresh data")
                }
            }
        }
        #[cfg(bundled_game_data)]
        if data.is_none() {
            match GameData::decode(
                include_bytes!(env!("MTL_JOBINES_BUNDLED_GAME_DATA")).as_slice(),
                key,
            ) {
                Ok(value) => {
                    eprintln!("Using bundled game data");
                    data = Some(value);
                }
                Err(error) => {
                    eprintln!("Bundled game data unavailable ({error}); checking disk cache")
                }
            }
        }
        if data.is_none() {
            match GameData::read(&cache_path, key) {
                Ok(value) => {
                    eprintln!("Using disk cache: {}", cache_path.display());
                    data = Some(value);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => eprintln!("Ignoring invalid disk cache: {error}"),
            }
        }
        let mut hermes = Self::from_city_data(city, data, &program)?;
        hermes.cache = Some((key, cache_path));
        Ok(hermes)
    }
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn embedded() -> Result<Self, String> {
        let parsed = opencity::parse(include_str!("../../opencity/examples/default_map.road"))
            .map_err(|error| error.to_string())?;

        let mut elevation_map: Option<opencity::heightmap::ElevationMap> = None;
        let (program, city) = expand(&parsed, |p| {
            City::from_program_with_elevation_loader(p, |spec| {
                if let Some(map) = &elevation_map {
                    return Ok(map.clone());
                }
                let map = match spec.path.as_str() {
                    "default_elevation.png" => opencity::heightmap::ElevationMap::from_png(
                        spec.clone(),
                        include_bytes!("../../opencity/examples/default_elevation.png"),
                    )?,
                    path => return Err(format!("Unknown embedded elevation map: {path}")),
                };
                elevation_map = Some(map.clone());
                Ok(map)
            })
        })?;
        #[cfg(bundled_game_data)]
        {
            let key = CacheKey::for_map(
                include_bytes!("../../opencity/examples/default_map.road"),
                include_bytes!("../../opencity/examples/default_elevation.png"),
            );
            match GameData::decode(
                include_bytes!(env!("MTL_JOBINES_BUNDLED_GAME_DATA")).as_slice(),
                key,
            ) {
                Ok(data) => return Self::from_city_data(city, Some(data), &program),
                Err(error) => {
                    eprintln!("Bundled game data unavailable ({error}); preparing fresh data")
                }
            }
        }
        Self::from_city_data(city, None, &program)
    }
    fn from_city_data(
        city: City,
        data: Option<GameData>,
        program: &opencity::Program,
    ) -> Result<Self, String> {
        let (minimap, prepared) = match data {
            Some(data) => (
                minimap::Minimap::from_pixels(&city, data.atlas),
                Some((data.world, data.details)),
            ),
            None => (minimap::Minimap::new(&city)?, None),
        };
        let router = routing::Router::new(&city, program);
        Ok(Self {
            router,
            navigation: routing::Navigation::default(),
            city: Arc::new(city),
            minimap,
            prepared,
            #[cfg(not(target_arch = "wasm32"))]
            cache: None,
        })
    }
    pub fn take_geometry(&mut self) -> (Mesh, Mesh) {
        if let Some(meshes) = self.prepared.take() {
            return meshes;
        }
        let world = self
            .city
            .mesh(Region::WholeMap, 0.04)
            .expect("static map mesh");
        let (mut details, _) = self
            .city
            .building_detail_parts(Region::WholeMap, &building_style::residential());
        details
            .vertices
            .extend(commerce_mesh::build(&self.city).vertices);
        eprintln!(
            "Prepared {} world vertices and {} detail vertices",
            world.vertices.len(),
            details.vertices.len()
        );
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((key, path)) = &self.cache {
            match game_data::write(path, *key, &world, &details, self.minimap.pixels()) {
                Ok(()) => eprintln!("Saved disk cache: {}", path.display()),
                Err(error) => {
                    eprintln!(
                        "Could not save disk cache ({error}); continuing with in-memory \
                            data"
                    )
                }
            }
        }
        (world, details)
    }
    pub(crate) fn prop_groups(&self) -> Vec<Vec<Mesh>> {
        let mut props = self.city.detail_objects(Region::WholeMap);
        let (_, frontage) = self
            .city
            .building_detail_parts(Region::WholeMap, &building_style::residential());
        props.extend(frontage);
        let mut groups: Vec<_> = props.into_iter().map(|mesh| vec![mesh]).collect();
        groups.extend(commerce_mesh::prop_groups(&self.city));
        groups
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_game_data(&mut self, path: &Path) -> Result<(), String> {
        let (world, details) = self.take_geometry();
        let key = self
            .cache
            .as_ref()
            .expect("native loaded map has a cache key")
            .0;
        game_data::write(path, key, &world, &details, self.minimap.pixels())
            .map_err(|e| e.to_string())
    }
    pub fn city(&self) -> &Arc<City> {
        &self.city
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn minimap_svg(&self) -> String {
        self.city.minimap_svg().expect("loaded map SVG")
    }
    pub fn update_minimap(&mut self, point: [f32; 2]) -> Option<&[u8]> {
        self.minimap.update(point)
    }
    pub fn select_destination(&mut self, uv: [f32; 2], position: [f32; 3]) {
        self.navigation
            .select(&self.router, self.minimap.world_point(uv), position.into());
    }
    pub fn clear_destination(&mut self) {
        self.navigation.clear();
    }
    pub fn update_navigation(&mut self, position: [f32; 3]) {
        self.navigation.update(&self.router, position.into());
    }
    pub fn route_revision(&self) -> u64 {
        self.navigation.revision
    }
    pub fn route_uvs(&self) -> Vec<[f32; 2]> {
        self.navigation
            .points
            .iter()
            .map(|&p| self.minimap.map_uv(p))
            .collect()
    }
    pub fn minimap_view(&self, point: [f32; 2], heading: f32) -> MinimapView {
        let mut view = self.minimap.view(point, heading);
        view.destination_uv = self
            .navigation
            .destination
            .map(|p| self.minimap.map_uv(p))
            .unwrap_or_default();
        view.route_status = self.navigation.status;
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencity::runtime::Region;

    #[test]
    fn embedded_map_matches_native_assets() {
        let embedded = Hermes::embedded().unwrap();
        let native = Hermes::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/opencity/examples/default_map.road"
        )))
        .unwrap();
        let (a, b) = (embedded.city(), native.city());
        assert_eq!(a.roads.len(), b.roads.len());
        assert_eq!(a.paths.len(), b.paths.len());
        assert_eq!(a.bounds, b.bounds);
        assert_eq!(
            a.query(Region::WholeMap).buildings.len(),
            b.query(Region::WholeMap).buildings.len()
        );
        for road in &a.roads {
            let p = road.segment.point_at(0.5);
            let point = [p.x, p.y];
            assert_eq!(
                a.elevation.as_ref().unwrap().height_at(point),
                b.elevation.as_ref().unwrap().height_at(point)
            );
            assert_eq!(
                a.surface_at(point, 100.).map(|s| s.height),
                b.surface_at(point, 100.).map(|s| s.height)
            );
        }
        assert_eq!(embedded.minimap_svg(), native.minimap_svg());
        assert_eq!(
            City::from_program_with_elevation_loader(
                &opencity::parse("elevation_map(\"missing.png\", 0, 0, 1, 1, 0, 1);").unwrap(),
                |_| Err("missing embedded asset".into()),
            )
            .err()
            .unwrap(),
            "missing embedded asset"
        );
    }
}

#[cfg(test)]
mod scaffold_tests {
    use super::*;
    #[test]
    fn game_dressing_preserves_every_shared_scaffold_proportion() {
        let input = opencity::parse(
            "road local: sidewalk carlane sidewalk; end
            street=local(-2,0,2,0);ground=land(-3,-2,3,-2,3,2,-3,2);elevation(ground,0.02);
            zone=frontage_zone(street,{use:mixed_use,side:both,start:0.1,end:0.9,
                depth:0.16,seed:42});
            residential_area(42,-2,-1,2,1,0,0,0);",
        )
        .unwrap();
        let (preview, _) =
            opencity::runtime::scaffold::expand(&input, |p| City::from_program(p, None)).unwrap();
        let (game, _) = expand(&input, |p| City::from_program(p, None)).unwrap();
        let a = opencity::runtime::scaffold::buildings(&preview);
        let b = opencity::runtime::scaffold::buildings(&game);
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
        for (a, b) in a.iter().zip(&b) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.feature.points, b.feature.points);
            assert_eq!(a.feature.height, b.feature.height);
            assert_eq!(a.feature.elevation, b.feature.elevation);
            assert_eq!(a.feature.front_edge, b.feature.front_edge);
            assert_eq!(a.roof, b.roof);
        }
    }
}
