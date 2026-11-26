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

pub struct SkiHill {
    cells: Box<[u8]>,
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
        Ok(Some(Self {
            cells: cells.into_boxed_slice(),
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
