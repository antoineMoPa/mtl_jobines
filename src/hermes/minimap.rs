use opencity::runtime::City;
use resvg::{tiny_skia, usvg};
pub const TEXTURE_SIZE: u32 = 4096;
const VISIBLE_SPAN: f32 = 4.;
#[derive(Clone, Copy)]
pub struct MinimapView {
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
    bounds: [f32; 4],
    span: f32,
    pixels: Vec<u8>,
    dirty: bool,
}
impl Minimap {
    pub fn new(city: &City) -> Result<Self, String> {
        let svg = city.minimap_svg()?;
        let tree =
            usvg::Tree::from_str(&svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            #[cfg(test)]
            svg,
            tree: Some(tree),
            bounds: city.bounds,
            span: (city.bounds[2] - city.bounds[0]).max(city.bounds[3] - city.bounds[1]),
            pixels: Vec::new(),
            dirty: true,
        })
    }
    #[cfg(test)]
    pub fn svg(&self) -> &str {
        &self.svg
    }
    pub fn from_pixels(city: &City, pixels: Vec<u8>) -> Self {
        assert_eq!(
            pixels.len(),
            TEXTURE_SIZE as usize * TEXTURE_SIZE as usize * 4
        );
        Self {
            #[cfg(test)]
            svg: String::new(),
            tree: None,
            bounds: city.bounds,
            span: (city.bounds[2] - city.bounds[0]).max(city.bounds[3] - city.bounds[1]),
            pixels,
            dirty: true,
        }
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
        if self.pixels.is_empty() {
            let mut pixmap = tiny_skia::Pixmap::new(TEXTURE_SIZE, TEXTURE_SIZE)
                .expect("fixed minimap texture size");
            pixmap.fill(tiny_skia::Color::from_rgba8(23, 62, 85, 255));
            let tree = self.tree.as_ref().expect("uncached map has an SVG tree");
            let scale = TEXTURE_SIZE as f32 / self.span * (self.bounds[2] - self.bounds[0])
                / tree.size().width();
            let transform = tiny_skia::Transform::from_row(
                scale,
                0.,
                0.,
                scale,
                (TEXTURE_SIZE as f32 - tree.size().width() * scale) * 0.5,
                (TEXTURE_SIZE as f32 - tree.size().height() * scale) * 0.5,
            );
            resvg::render(tree, transform, &mut pixmap.as_mut());
            self.pixels = pixmap.take();
        }
        &self.pixels
    }
    pub fn update(&mut self, _point: [f32; 2]) -> Option<&[u8]> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        Some(self.pixels())
    }
    pub fn view(&self, point: [f32; 2], heading: f32) -> MinimapView {
        MinimapView {
            uv_center: self.map_uv(point),
            uv_span: VISIBLE_SPAN / self.span,
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
            let pixels = map.pixels();
            for (uv, pavement) in samples {
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
