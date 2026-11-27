//! The editor's exact classification mask; tree geometry is separately shared/instanced.
use std::io::Cursor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Unmarked,
    Forest,
    Piste,
    IndividualSpruce,
}

/// RGB colors are exact category identifiers, never interpolated.
pub const AREA_COLORS: [[u8; 3]; 4] = [
    [238, 241, 245], // Unmarked: light gray
    [38, 112, 67],   // Forest: green
    [250, 204, 21],  // Piste: yellow
    [124, 58, 237],  // Individual spruce: purple
];

fn read_png(bytes: &[u8], color: png::ColorType) -> Result<Vec<u8>, String> {
    let mut reader = png::Decoder::new(Cursor::new(bytes))
        .read_info()
        .map_err(|e| e.to_string())?;
    let info = reader.info();
    if info.width != 180
        || info.height != 190
        || info.bit_depth != png::BitDepth::Eight
        || info.color_type != color
    {
        return Err(format!("ski hill PNG must be 180x190, 8-bit {color:?}"));
    }
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    pixels.truncate(frame.buffer_size());
    Ok(pixels)
}

pub fn read_area_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    read_png(bytes, png::ColorType::Rgb)?
        .chunks_exact(3)
        .map(|rgb| {
            AREA_COLORS
                .iter()
                .position(|color| color.as_slice() == rgb)
                .map(|kind| kind as u8)
                .ok_or_else(|| {
                    format!("unknown ski area color {rgb:?}; use the documented palette")
                })
        })
        .collect()
}

pub fn read_density_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    Ok(read_png(bytes, png::ColorType::Grayscale)?
        .into_iter()
        .map(|value| ((u16::from(value) * 100 + 127) / 255) as u8)
        .collect())
}

/// Maps the editor's exact integer cells and density percentages into lossless PNGs.
pub fn write_design_pngs(cells: &[u8], densities: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if cells.len() != 34200
        || densities.len() != 34200
        || cells.iter().any(|&v| v > 3)
        || densities.iter().any(|&v| v > 100)
    {
        return Err("invalid ski hill grid values".into());
    }
    let rgb: Vec<u8> = cells
        .iter()
        .flat_map(|&kind| AREA_COLORS[kind as usize])
        .collect();
    let gray: Vec<u8> = densities
        .iter()
        .map(|&v| ((u16::from(v) * 255 + 50) / 100) as u8)
        .collect();
    let encode = |pixels: &[u8], color| -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 180, 190);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(pixels).map_err(|e| e.to_string())?;
        }
        Ok(out)
    };
    Ok((
        encode(&rgb, png::ColorType::Rgb)?,
        encode(&gray, png::ColorType::Grayscale)?,
    ))
}

/// Shared wooden mast and a single downward-aimed hemispherical floodlight.
fn floodlight_mesh() -> &'static std::sync::Arc<opencity::runtime::Mesh> {
    static MESH: std::sync::OnceLock<std::sync::Arc<opencity::runtime::Mesh>> =
        std::sync::OnceLock::new();
    MESH.get_or_init(|| {
        use opencity::runtime::{Mesh, SurfaceKind};
        let mut mesh = Mesh::default();
        mesh.cylinder(
            [0., 0., 0.],
            0.002,
            0.09,
            [0.34, 0.22, 0.13],
            SurfaceKind::Prop,
        );
        let center = glam::Vec3::new(0.003, 0., 0.088);
        let axis = glam::Vec3::new(0.70710677, 0., -0.70710677);
        let up = glam::Vec3::new(0.70710677, 0., 0.70710677);
        let radius = 0.006;
        let point = |angle: f32, latitude: f32| {
            center
                + radius
                    * ((up * angle.cos() + glam::Vec3::Y * angle.sin()) * latitude.cos()
                        - axis * latitude.sin())
        };
        for i in 0..12 {
            let a = i as f32 * std::f32::consts::TAU / 12.;
            let b = (i + 1) as f32 * std::f32::consts::TAU / 12.;
            for ring in 0..3 {
                let lo = ring as f32 * std::f32::consts::FRAC_PI_2 / 3.;
                let hi = (ring + 1) as f32 * std::f32::consts::FRAC_PI_2 / 3.;
                mesh.quad(
                    [point(a, lo), point(b, lo), point(b, hi), point(a, hi)].map(|p| p.to_array()),
                    [0.19, 0.20, 0.20],
                    SurfaceKind::Prop,
                );
            }
            mesh.triangle(
                [center, point(a, 0.), point(b, 0.)].map(|p| p.to_array()),
                [0.91, 0.95, 1.],
                SurfaceKind::Light,
                [0.; 3],
            );
        }
        std::sync::Arc::new(mesh)
    })
}
pub(crate) fn is_floodlight(mesh: &std::sync::Arc<opencity::runtime::Mesh>) -> bool {
    std::sync::Arc::ptr_eq(mesh, floodlight_mesh())
}
pub(super) fn floodlight_parts(position: [f32; 3], yaw: f32) -> Vec<crate::props::Part> {
    vec![crate::props::Part {
        mesh: floodlight_mesh().clone(),
        transform: glam::Mat4::from_rotation_translation(
            glam::Quat::from_rotation_z(yaw),
            glam::Vec3::from_array(position),
        ),
        template: None,
        automatic: true,
        sign_profile: None,
    }]
}

pub struct SkiHill {
    cells: Box<[u8]>,
    base_chalet: Option<[f32; 2]>,
}
impl SkiHill {
    pub(super) fn read(program: &opencity::Program) -> Result<Option<Self>, String> {
        if !program
            .statements
            .iter()
            .any(|s| matches!(s,opencity::Statement::Call(c) if c.name=="ski_hill_design"))
        {
            return Ok(None);
        }
        let cells = read_area_png(include_bytes!("../../data/maps/mtl/ski-hill-areas.png"))?;
        let base_chalet = program
            .statements
            .iter()
            .find(|statement| {
                matches!(statement, opencity::Statement::Call(call)
                if call.name == "building" && call.assigns.iter().any(|name| name == "ski_chalet"))
            })
            .and_then(|statement| {
                let chalet = opencity::Program {
                    statements: vec![statement.clone()],
                };
                let footprint = opencity::project_landscape_features(&chalet)
                    .features
                    .into_iter()
                    .next()?
                    .points;
                let center = footprint
                    .iter()
                    .copied()
                    .map(glam::Vec2::from_array)
                    .sum::<glam::Vec2>()
                    / footprint.len() as f32;
                Some(center.to_array())
            });
        Ok(Some(Self {
            cells: cells.into_boxed_slice(),
            base_chalet,
        }))
    }
    pub fn area_at(&self, point: [f32; 2]) -> Option<Area> {
        if point.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let col = ((point[0] + 10.) * 10.).floor() as isize;
        let row = ((-76. - point[1]) * 10.).floor() as isize;
        if !(0..180).contains(&col) || !(0..190).contains(&row) {
            return None;
        }
        Some(match self.cells[row as usize * 180 + col as usize] {
            0 => Area::Unmarked,
            1 => Area::Forest,
            2 => Area::Piste,
            3 => Area::IndividualSpruce,
            _ => unreachable!("validated mask"),
        })
    }
    /// Edge cells lie just outside a piste and point toward its adjacent snow.
    fn piste_edge_cells(&self) -> Vec<([f32; 2], f32)> {
        let mut cells = Vec::new();
        for row in 0..190_i32 {
            for col in 0..180_i32 {
                if self.cells[row as usize * 180 + col as usize] == 2 {
                    continue;
                }
                let mut toward = glam::Vec2::ZERO;
                for (dc, dr) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let (x, y) = (col + dc, row + dr);
                    if (0..180).contains(&x)
                        && (0..190).contains(&y)
                        && self.cells[y as usize * 180 + x as usize] == 2
                    {
                        toward = glam::Vec2::new(dc as f32, -dr as f32);
                        break;
                    }
                }
                if toward != glam::Vec2::ZERO {
                    cells.push((
                        [
                            -10. + (col as f32 + 0.5) * 0.1,
                            -76. - (row as f32 + 0.5) * 0.1,
                        ],
                        toward.y.atan2(toward.x),
                    ));
                }
            }
        }
        cells
    }

    /// Space poles along the outside of the marked piste, aimed across its edge.
    pub(super) fn lamp_positions(
        &self,
        city: &opencity::runtime::City,
        lift_bases: impl IntoIterator<Item = [f32; 2]>,
    ) -> Vec<([f32; 3], f32)> {
        // Keep the chalet apron and lower loading areas clear of piste poles.
        let clearances: Vec<_> = self
            .base_chalet
            .into_iter()
            .map(|point| (point, 0.90_f32))
            .chain(lift_bases.into_iter().map(|point| (point, 0.85)))
            .collect();
        let mut positions: Vec<([f32; 3], f32)> = Vec::new();
        for (point, yaw) in self.piste_edge_cells() {
            let xy = glam::Vec2::from_array(point);
            if clearances.iter().any(|&(center, radius)| {
                xy.distance_squared(glam::Vec2::from_array(center)) < radius.powi(2)
            }) {
                continue;
            }
            if positions.iter().any(|(p, _)| {
                glam::Vec2::new(p[0] - point[0], p[1] - point[1]).length_squared()
                    < 0.20_f32.powi(2) - 1e-6
            }) {
                continue;
            }
            if let Some(surface) = city.surface_at(point, f32::INFINITY) {
                if surface.kind != opencity::runtime::SurfaceKind::Water {
                    positions.push(([point[0], point[1], surface.height], yaw));
                }
            }
        }
        positions
    }
    pub fn counts(&self) -> [usize; 4] {
        let mut counts = [0; 4];
        for &kind in &self.cells {
            counts[kind as usize] += 1;
        }
        counts
    }
    pub fn mask_bytes(&self) -> usize {
        self.cells.len()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn piste_lamps_stay_outside_the_runs_and_point_toward_the_edge() {
        let program = opencity::parse(
            "ski_hill_design(); land(-10,-95,8,-95,8,-76,-10,-76);
                ski_chalet = building(0.08, 4,-82.7,4.2,-82.7,4.2,-82.5,4,-82.5);",
        )
        .unwrap();
        let hill = SkiHill::read(&program).unwrap().unwrap();
        let city = opencity::runtime::City::from_program(&program, None).unwrap();
        assert!(
            glam::Vec2::from_array(hill.base_chalet.unwrap()).distance(glam::Vec2::new(4.1, -82.6))
                < 0.0001
        );
        let lamps = hill.lamp_positions(&city, [[3.8, -82.6]]);
        assert!(!lamps.is_empty());
        for (i, (position, yaw)) in lamps.iter().enumerate() {
            let point = [position[0], position[1]];
            assert!(glam::Vec2::from_array(point).distance(glam::Vec2::new(3.8, -82.6)) >= 0.85);
            assert!(glam::Vec2::from_array(point).distance(glam::Vec2::new(4.1, -82.6)) >= 0.90);
            assert_ne!(hill.area_at(point), Some(Area::Piste));
            assert!([(0.1, 0.), (-0.1, 0.), (0., 0.1), (0., -0.1)]
                .into_iter()
                .any(|(x, y)| hill.area_at([point[0] + x, point[1] + y]) == Some(Area::Piste)));
            let aimed = [point[0] + yaw.cos() * 0.1, point[1] + yaw.sin() * 0.1];
            assert_eq!(hill.area_at(aimed), Some(Area::Piste));
            for (other, _) in &lamps[..i] {
                let distance = glam::Vec2::new(point[0] - other[0], point[1] - other[1]).length();
                assert!(distance >= 0.199);
            }
        }
    }

    #[test]
    fn floodlight_instances_share_a_wooden_mast_and_oriented_lens() {
        let a = floodlight_parts([0., 0., 0.], 0.);
        let b = floodlight_parts([1., 2., 3.], std::f32::consts::FRAC_PI_2);
        assert!(is_floodlight(&a[0].mesh));
        assert!(std::sync::Arc::ptr_eq(&a[0].mesh, &b[0].mesh));
        assert!(a[0]
            .mesh
            .vertices
            .iter()
            .any(|v| v.surface[0] == opencity::runtime::SurfaceKind::Light as u32 as f32));
        let direction = b[0].transform.transform_vector3(glam::Vec3::X);
        assert!(direction.distance(glam::Vec3::Y) < 1e-5);
    }

    #[test]
    fn pngs_roundtrip_every_area_and_density_percentage() {
        let cells: Vec<_> = (0..34200).map(|i| (i % 4) as u8).collect();
        let densities: Vec<_> = (0..34200).map(|i| (i % 101) as u8).collect();
        let (areas, density) = write_design_pngs(&cells, &densities).unwrap();
        assert_eq!(read_area_png(&areas).unwrap(), cells);
        assert_eq!(read_density_png(&density).unwrap(), densities);
        assert!(write_design_pngs(&[4; 34200], &densities).is_err());
        assert!(write_design_pngs(&cells, &[101; 34200]).is_err());
        assert!(read_area_png(&density).is_err());
        assert!(read_area_png(&areas[..areas.len() / 2]).is_err());
    }

    #[test]
    fn unknown_palette_color_is_rejected() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 180, 190);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&vec![0; 34200 * 3]).unwrap();
        }
        assert!(read_area_png(&bytes)
            .unwrap_err()
            .contains("unknown ski area color"));
    }

    #[test]
    fn imported_mask_has_exact_coverage_and_rejects_outside_points() {
        let program = opencity::parse("ski_hill_design();").unwrap();
        let mask = SkiHill::read(&program).unwrap().unwrap();
        assert_eq!(mask.counts(), [23821, 5948, 4431, 0]);
        assert_eq!(mask.mask_bytes(), 34200);
        assert_eq!(mask.area_at([100., 0.]), None);
        assert_eq!(mask.area_at([f32::NAN, -82.]), None);
        assert!(SkiHill::read(&opencity::parse("").unwrap())
            .unwrap()
            .is_none());
    }
}
