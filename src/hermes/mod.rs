//! Static map/navigation access. OpenCity owns authored roads and geometry;
//! Hermes owns navigation, routing and the game’s map presentation.
mod building_style;
mod game_data;
mod minimap;
mod residential;
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
        let (mut program, areas) = residential::prepare(&parsed)?;
        let mut city = City::from_program(&program, path.parent())?;
        if !areas.is_empty() {
            let count = residential::populate(&mut program, &city, &areas);
            eprintln!(
                "Generated {count} stable residential buildings in {} areas",
                areas.len()
            );
            drop(city);
            city = City::from_program(&program, path.parent())?;
        }
        let elevation = city
            .elevation
            .as_ref()
            .map(|e| std::fs::read(path.parent().unwrap_or(Path::new(".")).join(&e.spec.path)))
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
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
        let (mut program, areas) = residential::prepare(&parsed)?;
        let load_elevation = |spec: &opencity::heightmap::ElevationMapSpec| match spec.path.as_str()
        {
            "default_elevation.png" => opencity::heightmap::ElevationMap::from_png(
                spec.clone(),
                include_bytes!("../../opencity/examples/default_elevation.png"),
            ),
            path => Err(format!("Unknown embedded elevation map: {path}")),
        };
        let mut city = City::from_program_with_elevation_loader(&program, load_elevation)?;
        if !areas.is_empty() {
            let count = residential::populate(&mut program, &city, &areas);
            eprintln!(
                "Generated {count} stable residential buildings in {} areas",
                areas.len()
            );
            drop(city);
            city = City::from_program_with_elevation_loader(&program, load_elevation)?;
        }
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
        let (details, _) = self
            .city
            .building_detail_parts(Region::WholeMap, &building_style::residential());
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
                    eprintln!("Could not save disk cache ({error}); continuing with in-memory data")
                }
            }
        }
        (world, details)
    }
    pub(crate) fn prop_meshes(&self) -> Vec<Mesh> {
        let mut props = self.city.detail_objects(Region::WholeMap);
        let (_, frontage) = self
            .city
            .building_detail_parts(Region::WholeMap, &building_style::residential());
        props.extend(frontage);
        props
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
