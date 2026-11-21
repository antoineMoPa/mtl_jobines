use opencity::runtime::City;
use resvg::{tiny_skia, usvg};
pub const TEXTURE_SIZE: u32 = 1024;
#[derive(Clone, Copy)]
pub enum MinimapScale {
    Ground,
    Flight,
}
impl MinimapScale {
    fn visible_span(self) -> f32 {
        match self {
            Self::Ground => 4.,  // 400 m across
            Self::Flight => 40., // 4 km across
        }
    }
}
#[derive(Clone, Copy)]
pub struct MinimapView {
    pub texture_region: [f32; 4],
    pub uv_center: [f32; 2],
    pub uv_span: f32,
    pub heading: f32,
    pub player_uv: [f32; 2],
    pub destination_uv: [f32; 2],
    pub route_status: super::RouteStatus,
}
pub struct Minimap {
    #[cfg(test)]
    svg: String,
    tree: Option<usvg::Tree>,
    pub(super) scale: MinimapScale,
    bounds: [f32; 4],
    span: f32,
    pixels: Vec<u8>,
    dirty: bool,
    local_pixels: Vec<u8>,
    local_region: Option<[f32; 4]>,
    active_region: [f32; 4],
}
impl Minimap {
    pub fn new(city: &City) -> Result<Self, String> {
        let svg = super::metro::navigation_svg(city)?;
        let tree =
            usvg::Tree::from_str(&svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            #[cfg(test)]
            svg,
            tree: Some(tree),
            scale: MinimapScale::Ground,
            bounds: city.bounds,
            span: (city.bounds[2] - city.bounds[0]).max(city.bounds[3] - city.bounds[1]),
            pixels: Vec::new(),
            dirty: true,
            local_pixels: Vec::new(),
            local_region: None,
            active_region: [0., 0., 1., 1.],
        })
    }
    #[cfg(test)]
    pub fn svg(&self) -> &str {
        &self.svg
    }
    pub fn from_pixels(city: &City, pixels: Vec<u8>) -> Result<Self, String> {
        assert_eq!(
            pixels.len(),
            TEXTURE_SIZE as usize * TEXTURE_SIZE as usize * 4
        );
        let mut map = Self::new(city)?;
        map.pixels = pixels;
        Ok(map)
    }
    pub fn map_uv(&self, point: [f32; 2]) -> [f32; 2] {
        [
            0.5 + (point[0] - (self.bounds[0] + self.bounds[2]) * 0.5) / self.span,
            0.5 - (point[1] - (self.bounds[1] + self.bounds[3]) * 0.5) / self.span,
        ]
    }
    pub fn world_point(&self, uv: [f32; 2]) -> [f32; 2] {
        [
            (self.bounds[0] + self.bounds[2]) * 0.5 + (uv[0] - 0.5) * self.span,
            (self.bounds[1] + self.bounds[3]) * 0.5 - (uv[1] - 0.5) * self.span,
        ]
    }
    pub fn pixels(&mut self) -> &[u8] {
        self.rasterize();
        &self.pixels
    }
    /// The on-screen minimap is small; edits favor a quick atlas over export detail.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn prepare_for_reload(&mut self) {
        self.rasterize();
    }
    fn rasterize(&mut self) {
        let size = TEXTURE_SIZE;
        if self.pixels.is_empty() {
            let mut pixmap =
                tiny_skia::Pixmap::new(size, size).expect("fixed minimap texture size");
            pixmap.fill(tiny_skia::Color::from_rgba8(23, 62, 85, 255));
            let tree = self.tree.as_ref().expect("uncached map has an SVG tree");
            let scale =
                size as f32 / self.span * (self.bounds[2] - self.bounds[0]) / tree.size().width();
            let transform = tiny_skia::Transform::from_row(
                scale,
                0.,
                0.,
                scale,
                (size as f32 - tree.size().width() * scale) * 0.5,
                (size as f32 - tree.size().height() * scale) * 0.5,
            );
            resvg::render(tree, transform, &mut pixmap.as_mut());
            self.pixels = pixmap.take();
        }
    }
    pub fn update(&mut self, _point: [f32; 2]) -> Option<&[u8]> {
        if !self.dirty && self.active_region == [0., 0., 1., 1.] {
            return None;
        }
        self.dirty = false;
        self.active_region = [0., 0., 1., 1.];
        Some(self.pixels())
    }
    /// Keep global navigation coordinates; only the displayed raster window moves.
    pub fn update_view(
        &mut self,
        point: [f32; 2],
        full_map: Option<([f32; 2], f32)>,
    ) -> Option<&[u8]> {
        let (center, span) = match full_map {
            Some((_, span)) if span >= 1. => return self.update(point),
            Some(view) => view,
            None => {
                let center = self.map_uv(point);
                let span = self.scale.visible_span() * 1.5 / self.span;
                if let Some(region) = self.local_region {
                    let margin = self.scale.visible_span() * 0.5 / self.span;
                    let inside = (0..2).all(|i| {
                        center[i] - margin >= region[i]
                            && center[i] + margin <= region[i] + region[2]
                    });
                    if inside && (region[2] - span).abs() < 1e-6 {
                        if self.active_region == region {
                            return None;
                        }
                        self.active_region = region;
                        return Some(&self.local_pixels);
                    }
                }
                (center, span)
            }
        };
        let region = [center[0] - span * 0.5, center[1] - span * 0.5, span, span];
        if self.local_region == Some(region) {
            if self.active_region == region {
                return None;
            }
        } else {
            let size = TEXTURE_SIZE;
            // Reuse the pixel allocation when recentering instead of doubling peak memory.
            let pixels = std::mem::take(&mut self.local_pixels);
            let mut pixmap = tiny_skia::Pixmap::from_vec(
                pixels,
                tiny_skia::IntSize::from_wh(size, size).unwrap(),
            )
            .unwrap_or_else(|| tiny_skia::Pixmap::new(size, size).unwrap());
            pixmap.fill(tiny_skia::Color::from_rgba8(23, 62, 85, 255));
            let tree = self.tree.as_ref().expect("map SVG tree");
            let scale = size as f32 / (self.span * span) * (self.bounds[2] - self.bounds[0])
                / tree.size().width();
            let transform = tiny_skia::Transform::from_row(
                scale,
                0.,
                0.,
                scale,
                (size as f32 / span - tree.size().width() * scale) * 0.5
                    - region[0] * size as f32 / span,
                (size as f32 / span - tree.size().height() * scale) * 0.5
                    - region[1] * size as f32 / span,
            );
            resvg::render(tree, transform, &mut pixmap.as_mut());
            self.local_pixels = pixmap.take();
            self.local_region = Some(region);
        }
        self.active_region = region;
        Some(&self.local_pixels)
    }
    pub fn view(&self, point: [f32; 2], heading: f32) -> MinimapView {
        MinimapView {
            texture_region: self.active_region,
            uv_center: self.map_uv(point),
            uv_span: self.scale.visible_span() / self.span,
            heading,
            player_uv: self.map_uv(point),
            destination_uv: [0.; 2],
            route_status: super::RouteStatus::Inactive,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_raster_reuses_pixels_until_player_leaves_margin_and_keeps_global_uvs() {
        let city = map();
        let mut map = Minimap::new(&city).unwrap();
        let uv = map.map_uv([0., 0.]);
        assert!(map.update_view([0., 0.], None).is_some());
        assert!(map.pixels.is_empty(), "overview should remain lazy");
        let allocation = map.local_pixels.as_ptr();
        let region = map.view([0., 0.], 0.).texture_region;
        assert_eq!(map.view([0., 0.], 0.).player_uv, uv);
        assert!(map.update_view([0.5, 0.], None).is_none());
        assert!(map.update_view([1.1, 0.], None).is_some());
        assert_ne!(map.active_region, region);
        assert_eq!(map.local_pixels.as_ptr(), allocation);
        assert!(map.update_view([1.1, 0.], Some(([0.5, 0.5], 1.))).is_some());
        assert_eq!(map.active_region, [0., 0., 1., 1.]);
        assert!(map.update_view([1.1, 0.], None).is_some());
        assert_eq!(map.local_pixels.as_ptr(), allocation);
    }
    #[test]
    fn flight_view_shows_runway_alignment_from_a_kilometre_out() {
        let city = City::from_program(
            &opencity::parse("strip = concrete(0,0,16,0,16,0.65,0,0.65);").unwrap(),
            None,
        )
        .unwrap();
        let mut map = Minimap::new(&city).unwrap();
        let approach = [-10., 0.325];
        let visible = |view: MinimapView, uv: [f32; 2]| {
            (uv[0] - view.uv_center[0]).hypot(uv[1] - view.uv_center[1]) < view.uv_span * 0.5
        };
        let threshold = map.map_uv([0., 0.325]);
        let midpoint = map.map_uv([8., 0.325]);
        map.update_view(approach, None).unwrap();
        let ground = map.view(approach, 0.);
        assert!(!visible(ground, threshold));

        map.scale = MinimapScale::Flight;
        map.update_view(approach, None).unwrap();
        let flight = map.view(approach, 0.);
        assert!(visible(flight, threshold) && visible(flight, midpoint));
        let [left, top, width, height] = flight.texture_region;
        let x = ((midpoint[0] - left) / width * TEXTURE_SIZE as f32) as usize;
        let y = ((midpoint[1] - top) / height * TEXTURE_SIZE as f32) as usize;
        let pixel = (y * TEXTURE_SIZE as usize + x) * 4;
        assert_eq!(&map.local_pixels[pixel..pixel + 3], &[150, 157, 148]);
        assert!(map.update_view(approach, None).is_none());

        map.scale = MinimapScale::Ground;
        map.update_view(approach, None).unwrap();
        assert_eq!(map.view(approach, 0.).texture_region, ground.texture_region);
    }
    #[test]
    fn zoomed_raster_draws_road_at_its_global_coordinate() {
        let city = map();
        let mut map = Minimap::new(&city).unwrap();
        let uv = map.map_uv([0.5, 0.]);
        let pixels = map.update_view([0., 0.], Some((uv, 0.25))).unwrap();
        let i = ((TEXTURE_SIZE / 2 * TEXTURE_SIZE + TEXTURE_SIZE / 2) * 4) as usize;
        assert!(pixels[i] > 180 && pixels[i + 1] > 180);
        assert_eq!(map.view([0.5, 0.], 0.).player_uv, uv);
        assert!(map.update_view([0., 0.], Some((uv, 0.25))).is_none());
    }
    #[test]
    fn atlas_world_mapping_round_trips_including_padded_rectangular_bounds() {
        let map = Minimap::new(&map()).unwrap();
        for point in [[0., 0.], [-2., 0.], [2., 0.], [1., -0.5]] {
            let actual = map.world_point(map.map_uv(point));
            assert!((actual[0] - point[0]).abs() < 1e-5);
            assert!((actual[1] - point[1]).abs() < 1e-5);
        }
    }
    fn map() -> City {
        City::from_program(
            &opencity::parse("road street: carlane; end street(-2,0,2,0);").unwrap(),
            None,
        )
        .unwrap()
    }
    #[test]
    fn svg_and_atlas_preserve_north_up_coordinates_without_rebuilds() {
        let city = map();
        let mut map = Minimap::new(&city).unwrap();
        assert!(map.svg().contains("<path"));
        let origin = map.map_uv([0., 0.]);
        assert!(map.map_uv([0., 1.])[1] < origin[1]);
        assert!(map.map_uv([1., 0.])[0] > origin[0]);
        assert_eq!(
            map.update([0., 0.]).unwrap().len(),
            TEXTURE_SIZE as usize * TEXTURE_SIZE as usize * 4
        );
        for point in [[0.1, 0.], [0.6, 0.], [100., -100.]] {
            assert!(map.update(point).is_none());
            assert_eq!(map.view(point, 1.).uv_center, map.map_uv(point));
        }
    }
    #[test]
    fn exported_road_is_visible_at_the_player_coordinate() {
        let city = map();
        let mut map = Minimap::new(&city).unwrap();
        let uv = map.map_uv([0., 0.]);
        let pixels = map.update([0., 0.]).unwrap();
        let i = ((uv[1] * TEXTURE_SIZE as f32) as u32 * TEXTURE_SIZE
            + (uv[0] * TEXTURE_SIZE as f32) as u32) as usize
            * 4;
        assert!(
            pixels[i] > 180 && pixels[i + 1] > 180,
            "road should be light in actual raster"
        );
    }
    #[test]
    fn quick_reload_atlas_keeps_world_mapping_and_has_no_deferred_raster_work() {
        let city = map();
        let mut map = Minimap::new(&city).unwrap();
        map.prepare_for_reload();
        let pixels = map.pixels.as_ptr();
        let uv = map.map_uv([0., 0.]);
        let atlas = map.update([0., 0.]).unwrap();
        assert_eq!(atlas.as_ptr(), pixels);
        assert_eq!(atlas.len(), (TEXTURE_SIZE * TEXTURE_SIZE * 4) as usize);
        let i = ((uv[1] * TEXTURE_SIZE as f32) as usize * TEXTURE_SIZE as usize
            + (uv[0] * TEXTURE_SIZE as f32) as usize)
            * 4;
        assert!(atlas[i] > 180 && atlas[i + 1] > 180);
        assert!(map.update([0., 0.]).is_none());
    }
    #[test]
    fn placed_three_way_junctions_fill_the_gap_between_ports_in_every_orientation() {
        let directions = [
            ("east", 1., 0.),
            ("north", 0., 1.),
            ("west", -1., 0.),
            ("south", 0., -1.),
        ];
        for closed in 0..4 {
            let mut source = String::from(
                "road street: sidewalk carlane carlane sidewalk; end center = map_point(0,0);",
            );
            let mut ports = String::new();
            for (index, (name, dx, dy)) in directions.iter().enumerate() {
                if index == closed {
                    continue;
                }
                source.push_str(&format!(
                    "p{index}=map_point({},{}); outer{index}=map_point({dx},{dy}); \
                        street(p{index},{},{},{},{},outer{index});",
                    dx * 0.2,
                    dy * 0.2,
                    dx * 0.4,
                    dy * 0.4,
                    dx * 0.8,
                    dy * 0.8
                ));
                ports.push_str(&format!(",{name},p{index}"));
            }
            source.push_str(&format!("intersection3(center,street{ports});"));
            let city = City::from_program(&opencity::parse(&source).unwrap(), None).unwrap();
            assert_eq!(city.junctions.len(), 1);
            assert!(
                city.components
                    .iter()
                    .any(|component| component.id == "profile:center"),
                "T junctions use shared curved profile geometry"
            );
            let mut map = Minimap::new(&city).unwrap();
            let samples: Vec<_> = std::iter::once(([0.00137, 0.00219], true))
                .chain(directions.iter().enumerate().map(|(index, (_, dx, dy))| {
                    (
                        [
                            dx * if index == closed { 0.06 } else { 0.15 } + 0.00137,
                            dy * if index == closed { 0.06 } else { 0.15 } + 0.00219,
                        ],
                        index != closed,
                    )
                }))
                .map(|(point, pavement)| (map.map_uv(point), pavement))
                .collect();
            // Inspect junction detail through the zoomed raster, not the overview.
            let center = map.map_uv([0., 0.]);
            let span = 0.25;
            let pixels = map.update_view([0., 0.], Some((center, span))).unwrap();
            for (uv, pavement) in samples {
                let uv = [
                    (uv[0] - center[0]) / span + 0.5,
                    (uv[1] - center[1]) / span + 0.5,
                ];
                let i = ((uv[1] * TEXTURE_SIZE as f32) as usize * TEXTURE_SIZE as usize
                    + (uv[0] * TEXTURE_SIZE as f32) as usize)
                    * 4;
                let expected = if pavement {
                    [226, 229, 210]
                } else {
                    [167, 175, 161]
                };
                assert_eq!(
                    &pixels[i..i + 3],
                    &expected,
                    "closed direction {}, uv {uv:?}",
                    directions[closed].0
                );
            }
        }
    }
}
