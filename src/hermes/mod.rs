//! Static map/navigation access. OpenCity owns authored roads and geometry;
//! Hermes owns navigation, routing and the game’s map presentation.
mod airport_fence;
mod building_style;
pub mod commerce_mesh;
mod commerce_props;
mod commercial;
mod details;
pub mod external_models;
mod game_data;
pub mod landmark_destinations;
pub mod landmarks;
pub use landmark_destinations::Landmark;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod highway_tests;
#[cfg(test)]
mod intersection_tests;
pub mod metro;
mod minimap;
mod mission_sites;
mod orange_sign;
mod street_bins;
pub mod street_props;
mod taxi_runs;

mod route_signal;
mod routing;
pub use route_signal::RouteSignal;
pub mod traffic_paths;
pub mod walking_paths;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data))]
use game_data::CacheKey;
use game_data::GameData;
pub use minimap::MinimapView;
pub use minimap::TEXTURE_SIZE;
use opencity::runtime::City;
use opencity::runtime::{Mesh, Region};
pub use routing::RouteStatus;
use std::path::Path;
use std::sync::Arc;
/// Both platforms expand the same source, differing only in elevation asset access.
fn expand(
    source: &opencity::Program,
    mut compile: impl FnMut(&opencity::Program) -> Result<City, String>,
) -> Result<(opencity::Program, City), String> {
    let frontage = opencity::project_frontage_zones(source);
    let residential = source
        .statements
        .iter()
        .any(|s| matches!(s,opencity::Statement::Call(c)if c.name=="residential_area"));
    let mut program = if frontage.zones.is_empty() && frontage.errors.is_empty() && !residential {
        source.clone()
    } else {
        opencity::runtime::scaffold::expand_program(source, &mut compile)?
    };
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
        let height = match call.args.first() {
            Some(Argument::Number(value)) => value.parse::<f32>().unwrap_or(0.),
            _ => 0.,
        };
        let material = if height >= 0.24 {
            match (seed >> 16) % 4 {
                0..=1 => "glass",
                2 => "concrete",
                _ => "limestone",
            }
        } else if height <= 0.085 {
            match (seed >> 16) % 4 {
                0 => "pale_brick",
                1 => "buff_brick",
                2 => "brown_brick",
                _ => "red_brick",
            }
        } else {
            match (seed >> 16) % 10 {
                0..=4 => "red_brick",
                5..=6 => "brown_brick",
                7 => "buff_brick",
                8 => "limestone",
                _ => "pale_brick",
            }
        };
        if let Some(Argument::Object(options)) = call.args.get_mut(1) {
            if let Some((_, value)) = options.iter_mut().find(|(key, _)| key == "material") {
                *value = Argument::Identifier(material.into());
            }
        }
    }
}
// Developed-block interiors keep their shader facades and full collision shells;
// the authored street frontage receives balconies, stairs and garden objects.
fn detailed_frontage(feature: &opencity::LandscapeFeature) -> bool {
    !feature
        .name
        .as_deref()
        .is_some_and(|name| name.starts_with("generated_area_"))
}
pub struct Hermes {
    pub(crate) wildlife: Vec<crate::dynamic_world::wildlife::Home>,
    pub(crate) wildlife_habitats: crate::dynamic_world::wildlife::Habitats,
    authored_props: Vec<crate::props::Entry>,
    default_start: Option<crate::dynamic_world::SpawnPoint>,
    walking_start: Option<crate::dynamic_world::SpawnPoint>,
    pub(crate) boat_start: Option<crate::dynamic_world::SpawnPoint>,
    mission_sites: mission_sites::MissionSites,
    taxi_run: Option<taxi_runs::TaxiRun>,
    taxi_seed: u64,
    parked_vehicles: Vec<crate::dynamic_world::parked::ParkedVehicle>,
    city: Arc<City>,
    minimap: minimap::Minimap,
    prepared: Option<(Mesh, Mesh)>,
    details: details::Prepared,
    landmark_mesh: Mesh,
    pub external_models: Vec<external_models::instances::ModelInstances>,
    router: routing::Router,
    traffic_paths: Arc<traffic_paths::TrafficPaths>,
    walking_paths: Arc<walking_paths::WalkingPaths>,
    navigation: routing::Navigation,
    #[cfg(not(target_arch = "wasm32"))]
    cache: Option<(CacheKey, std::path::PathBuf)>,
}
impl Hermes {
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_map() -> Self {
        Self::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end
            street(-1,0,1,0); land(-2,-2,2,-2,2,2,-2,2);",
        )
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_map_from_source(source: &str) -> Self {
        let program = opencity::parse(source).unwrap();
        let city = City::from_program(&program, None).unwrap();
        Self::from_city_data(city, None, &program).unwrap()
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
        Self::from_map_bytes(&source, &elevation, packaged, path.parent())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_map_bytes(
        source: &[u8],
        elevation: &[u8],
        packaged: Option<&Path>,
        model_base: Option<&Path>,
    ) -> Result<Self, String> {
        let phase = crate::telemetry::span("map.parse");
        let parsed = opencity::parse(std::str::from_utf8(source).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let elevation_spec = opencity::heightmap::ElevationMapSpec::from_program(&parsed)?;
        // All expansion stages retain the authored terrain. Decode it once,
        // sharing the source bytes with the cache fingerprint below.
        drop(phase);
        let phase = crate::telemetry::span("map.elevation");
        let elevation_map = elevation_spec
            .map(|spec| opencity::heightmap::ElevationMap::from_png(spec, &elevation))
            .transpose()?;
        drop(phase);
        let phase = crate::telemetry::span("map.expand");
        let (program, city) = expand(&parsed, |p| {
            City::from_program_with_elevation_loader(p, |_| {
                elevation_map
                    .clone()
                    .ok_or_else(|| "Missing elevation asset".into())
            })
        })?;
        drop(phase);
        let phase = crate::telemetry::span("map.cache");
        let key = CacheKey::for_map(source, elevation);
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
                    game_data::maintain(&cache_path);
                    data = Some(value);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => eprintln!("Ignoring invalid disk cache: {error}"),
            }
        }
        drop(phase);
        let phase = crate::telemetry::span("map.world_data");
        let mut hermes = Self::from_city_data(city, data, &program)?;
        drop(phase);
        let _phase = crate::telemetry::span("map.models");
        hermes
            .external_models
            .extend(external_models::instances::load(
                &hermes.city,
                model_base.unwrap_or(Path::new(".")),
            )?);
        hermes.cache = Some((key, cache_path));
        Ok(hermes)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn embedded() -> Result<Self, String> {
        let parsed = opencity::parse(include_str!("../../data/maps/mtl/map.road"))
            .map_err(|error| error.to_string())?;

        let mut elevation_map: Option<opencity::heightmap::ElevationMap> = None;
        let (program, city) = expand(&parsed, |p| {
            City::from_program_with_elevation_loader(p, |spec| {
                if let Some(map) = &elevation_map {
                    return Ok(map.clone());
                }
                let map = match spec.path.as_str() {
                    "elevation.png" => opencity::heightmap::ElevationMap::from_png(
                        spec.clone(),
                        include_bytes!("../../data/maps/mtl/elevation.png"),
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
                include_bytes!("../../data/maps/mtl/map.road"),
                include_bytes!("../../data/maps/mtl/elevation.png"),
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
                minimap::Minimap::from_pixels(&city, data.atlas)?,
                Some((data.world, data.details)),
            ),
            None => (minimap::Minimap::new(&city)?, None),
        };
        let router = routing::Router::new(&city, program);
        let landmark_mesh = landmarks::build(&city)?;
        let authored_props = crate::props::read(program)?;
        for entry in &authored_props {
            entry.placements(&city)?;
        }
        #[cfg(target_arch = "wasm32")]
        let external_models = external_models::instances::load(&city, Path::new("."))?;
        #[cfg(not(target_arch = "wasm32"))]
        let external_models = Vec::new();
        let mut external_models = external_models;
        if let Some(stations) = metro::MetroMap::from_city(&city).scenery()? {
            external_models.push(stations);
        }
        Ok(Self {
            wildlife_habitats: crate::dynamic_world::wildlife::Habitats::read(program, &city),
            wildlife: crate::dynamic_world::wildlife::Home::read(program, &city)?,
            authored_props,
            default_start: crate::dynamic_world::SpawnPoint::from_program(program)?,
            walking_start: crate::dynamic_world::SpawnPoint::walking_from_program(program)?,
            boat_start: crate::dynamic_world::SpawnPoint::from_named_program(
                program,
                "boat_start",
            )?,
            mission_sites: mission_sites::MissionSites::from_program(program),
            taxi_run: None,
            taxi_seed: web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map_or(1, |time| time.as_nanos() as u64 | 1),
            parked_vehicles: crate::dynamic_world::parked::ParkedVehicle::from_program(program)?,
            traffic_paths: Arc::new(traffic_paths::TrafficPaths::new(&city, program)),
            walking_paths: Arc::new(walking_paths::WalkingPaths::new(&city)),
            router,
            navigation: routing::Navigation::default(),
            city: Arc::new(city),
            minimap,
            prepared,
            details: details::Prepared::default(),
            landmark_mesh,
            external_models,
            #[cfg(not(target_arch = "wasm32"))]
            cache: None,
        })
    }
    pub fn default_start(&self) -> Option<crate::dynamic_world::SpawnPoint> {
        self.default_start
    }
    pub fn walking_start(&self) -> Option<crate::dynamic_world::SpawnPoint> {
        self.walking_start
    }
    pub fn mission_site(&self, site: crate::kairos::SiteId) -> Option<[f32; 2]> {
        use crate::kairos::SiteId;
        match site {
            SiteId::TaxiPickup => self
                .taxi_run
                .map(|run| run.pickup.road.truncate().to_array()),
            SiteId::TaxiDropoff => self
                .taxi_run
                .map(|run| run.dropoff.road.truncate().to_array()),
            _ => self.mission_sites.get(site),
        }
    }
    pub fn mission_waiting_point(&self, site: crate::kairos::SiteId) -> Option<glam::Vec3> {
        use crate::kairos::SiteId;
        match site {
            SiteId::TaxiPickup => return self.taxi_run.map(|run| run.pickup.sidewalk),
            SiteId::TaxiDropoff => return self.taxi_run.map(|run| run.dropoff.sidewalk),
            _ => {}
        }
        let point = self.mission_site(site)?;
        self.mission_sites.waiting_point(&self.walking_paths, point)
    }
    pub fn select_mission_destination(
        &mut self,
        site: crate::kairos::SiteId,
        player_position: [f32; 3],
    ) -> bool {
        use crate::kairos::SiteId;
        if matches!(site, SiteId::TaxiPickup | SiteId::TaxiDropoff) {
            let Some(run) = self.taxi_run else {
                return false;
            };
            let road = if site == SiteId::TaxiPickup {
                run.pickup.road
            } else {
                run.dropoff.road
            };
            self.navigation
                .select_road_point(&self.router, road, player_position.into());
            return self.navigation.status != RouteStatus::Unreachable;
        }
        let Some(point) = self.mission_site(site) else {
            return false;
        };
        self.select_destination(self.minimap.map_uv(point), player_position);
        self.navigation.status != RouteStatus::Unreachable
    }
    pub(crate) fn parked_vehicles(&self) -> &[crate::dynamic_world::parked::ParkedVehicle] {
        &self.parked_vehicles
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn prepare_for_reload(&mut self) {
        // Accepted authoring edits should not wait for a compressed disk-cache write.
        let cache = self.cache.take();
        let geometry = self.take_geometry();
        self.cache = cache;
        self.prepared = Some(geometry);
        self.minimap.prepare_for_reload();
    }
    pub fn take_geometry(&mut self) -> (Mesh, Mesh) {
        let _span = crate::telemetry::span("map.geometry");
        if let Some(meshes) = self.prepared.take() {
            return meshes;
        }
        let mut world = self
            .city
            .mesh_with_external_models(Region::WholeMap, 0.04)
            .expect("static map mesh");
        metro::MetroMap::from_city(&self.city).open_portals(&mut world);
        world.vertices.append(&mut self.landmark_mesh.vertices);
        crate::dynamic_world::airport::append_scenery(&self.city, &mut world);
        let details = self.details.take_scenery(&self.city);
        eprintln!(
            "Prepared {} world vertices and {} detail vertices",
            world.vertices.len(),
            details.vertices.len()
        );
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((key, path)) = &self.cache {
            match game_data::write(path, *key, &world, &details, self.minimap.pixels()) {
                Ok(()) => {
                    eprintln!("Saved disk cache: {}", path.display());
                    game_data::maintain(path);
                }
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
    #[cfg(test)]
    pub fn prop_groups(&self) -> Vec<Vec<Mesh>> {
        let mut groups = street_props::groups(&self.city);
        let (_, frontage) = self.city.building_detail_parts_matching(
            Region::WholeMap,
            &building_style::residential(),
            detailed_frontage,
        );
        groups.extend(frontage.into_iter().map(|mesh| vec![mesh]));
        groups.extend(commerce_mesh::prop_groups(&self.city));
        groups.extend(orange_sign::meshes(&self.city));
        groups.extend(crate::dynamic_world::airport::prop_groups(&self.city));
        groups.extend(
            airport_fence::panels(&self.city)
                .into_iter()
                .map(|panel| vec![panel]),
        );
        groups
    }
    pub fn prop_parts(&self) -> Vec<Vec<crate::props::Part>> {
        let mut groups = street_props::parts(&self.city);
        groups.extend(self.details.take_props(&self.city, self.prepared.is_none()));
        groups.extend(orange_sign::parts(&self.city));
        groups.extend(
            crate::props::visible_placements(&self.authored_props, &self.city)
                .into_iter()
                .map(|(_, _, p)| p.parts()),
        );
        groups.extend(
            crate::dynamic_world::airport::prop_groups(&self.city)
                .into_iter()
                .map(|parts| parts.into_iter().map(crate::props::Part::baked).collect()),
        );
        groups.extend(
            airport_fence::panels(&self.city)
                .into_iter()
                .map(|panel| vec![crate::props::Part::baked(panel)]),
        );
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
    pub fn traffic_paths(&self) -> &Arc<traffic_paths::TrafficPaths> {
        &self.traffic_paths
    }
    pub fn walking_paths(&self) -> &Arc<walking_paths::WalkingPaths> {
        &self.walking_paths
    }
    pub fn city(&self) -> &Arc<City> {
        &self.city
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn minimap_svg(&self) -> String {
        metro::navigation_svg(&self.city).expect("loaded map SVG")
    }
    pub fn update_minimap_view(
        &mut self,
        point: [f32; 2],
        full_map: Option<([f32; 2], f32)>,
    ) -> Option<&[u8]> {
        self.minimap.update_view(point, full_map)
    }
    pub fn update_minimap(&mut self, point: [f32; 2]) -> Option<&[u8]> {
        self.minimap.update(point)
    }
    pub fn select_destination(&mut self, uv: [f32; 2], position: [f32; 3]) {
        self.navigation
            .select(&self.router, self.minimap.world_point(uv), position.into());
    }
    pub fn map_world_point(&self, uv: [f32; 2]) -> [f32; 2] {
        self.minimap.world_point(uv)
    }
    pub fn clear_destination(&mut self) {
        self.navigation.clear();
    }
    pub fn update_navigation(&mut self, position: [f32; 3]) {
        self.navigation.update(&self.router, position.into());
    }
    pub fn route_status(&self) -> RouteStatus {
        self.navigation.status
    }
    pub(crate) fn road_route(&self) -> &[glam::Vec3] {
        &self.navigation.points
    }
    pub fn route_revision(&self) -> u64 {
        self.navigation.revision
    }
    pub fn route_uvs(&self) -> Vec<[f32; 2]> {
        self.navigation
            .points
            .iter()
            .map(|p| self.minimap.map_uv(p.truncate().to_array()))
            .collect()
    }
    pub fn map_uv(&self, point: [f32; 2]) -> [f32; 2] {
        self.minimap.map_uv(point)
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
