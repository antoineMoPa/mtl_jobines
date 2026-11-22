use super::Result;
use std::{fs, path::Path};

fn srgb_to_linear(value: u8) -> f32 {
    let v = value as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let v = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(super) fn bake(root: &Path) -> Result<()> {
    let directory = root.join("assets/terrain");
    let mut reader = png::Decoder::new(fs::File::open(directory.join("grass.png"))?).read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels)?;
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    assert_eq!(info.width, info.height, "grass base level must be square");
    let mut levels = vec![(info.width, info.height, pixels)];
    while levels.last().is_some_and(|(w, h, _)| *w > 1 || *h > 1) {
        let (w, h, source) = levels.last().unwrap();
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut target = vec![255; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                // Integer source ranges also retain the last row/column of odd mips.
                let (x0, x1) = (x * w / nw, (x + 1) * w / nw);
                let (y0, y1) = (y * h / nh, (y + 1) * h / nh);
                for c in 0..3 {
                    let mut sum = 0.0;
                    for sy in y0..y1 {
                        for sx in x0..x1 {
                            sum += srgb_to_linear(source[((sy * w + sx) * 4 + c) as usize]);
                        }
                    }
                    target[((y * nw + x) * 4 + c) as usize] =
                        linear_to_srgb(sum / ((x1 - x0) * (y1 - y0)) as f32);
                }
            }
        }
        levels.push((nw, nh, target));
    }
    // Store complete levels consecutively in one PNG, padding only its final row.
    let mut packed: Vec<u8> = levels.into_iter().flat_map(|(_, _, bytes)| bytes).collect();
    let row_bytes = info.width as usize * 4;
    let rows = packed.len().div_ceil(row_bytes);
    packed.resize(rows * row_bytes, 0);
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, info.width, rows as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Best);
    encoder.set_filter(png::FilterType::Paeth);
    encoder.write_header()?.write_image_data(&packed)?;
    let output = directory.join("grass-mips.png");
    if fs::read(&output).ok().as_ref() != Some(&png) {
        fs::write(output, png)?;
    }
    Ok(())
}
