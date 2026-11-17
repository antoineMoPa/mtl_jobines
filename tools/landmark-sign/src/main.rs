//! Bake the flour mill's two-sided sign into its compact, texture-free mesh.
//! Run from the repository root: cargo run --manifest-path tools/landmark-sign/Cargo.toml
use std::{fs, path::Path};
use ttf_parser::{Face, OutlineBuilder};

// Keep raised lettering clear of the panel even after placement at city coordinates
// and depth-buffer quantization at distant camera positions.
const LETTERING_STANDOFF: f32 = 0.02;

#[derive(Default)]
struct Outline(Vec<Vec<[f64; 2]>>);
impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(vec![[x as f64, y as f64]]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.last_mut().unwrap().push([x as f64, y as f64]);
    }
    fn quad_to(&mut self, x: f32, y: f32, ex: f32, ey: f32) {
        let p = *self.0.last().unwrap().last().unwrap();
        for i in 1..=12 {
            let t = i as f64 / 12.;
            let u = 1. - t;
            self.0.last_mut().unwrap().push([
                u * u * p[0] + 2. * u * t * x as f64 + t * t * ex as f64,
                u * u * p[1] + 2. * u * t * y as f64 + t * t * ey as f64,
            ]);
        }
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        panic!("Expected quadratic TrueType outlines");
    }
    fn close(&mut self) {}
}
fn area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum::<f64>()
        .abs()
}
fn triangle(out: &mut Vec<u8>, mut p: [[f32; 3]; 3], side: f32, color: [f32; 3]) {
    let cross_y =
        (p[1][2] - p[0][2]) * (p[2][0] - p[0][0]) - (p[1][0] - p[0][0]) * (p[2][2] - p[0][2]);
    if cross_y * side < 0. {
        p.swap(1, 2);
    }
    for p in p {
        for v in [
            p[0], p[1], p[2], 0., side, 0., color[0], color[1], color[2], 1.,
        ] {
            out.extend(v.to_le_bytes());
        }
    }
}
fn main() {
    let path = Path::new("data/landmarks/25.mesh");
    let mut bytes = fs::read(path).unwrap();
    // Retain only the original model, making repeated baking byte-identical.
    // Original LMK1 vertex count is recorded before adding our sign geometry.
    const ORIGINAL_VERTICES: usize = 4866;
    bytes.truncate(32 + ORIGINAL_VERTICES * 40);
    let font = fs::read("assets/fonts/pt-sans-bold.ttf").unwrap();
    let face = Face::parse(&font, 0).unwrap();
    for side in [-1., 1.] {
        let y = if side < 0. { -0.39 } else { -0.32 };
        let panel = [
            [-0.68, y, 2.085],
            [0.18, y, 2.085],
            [0.18, y, 2.526],
            [-0.68, y, 2.526],
        ];
        for t in [[0, 1, 2], [0, 2, 3]] {
            triangle(&mut bytes, t.map(|i| panel[i]), side, [0.98; 3]);
        }
        for (text, bottom) in [("FARINE", 2.355), ("FIVE ROSES", 2.15)] {
            let glyphs: Vec<_> = text.chars().map(|c| face.glyph_index(c).unwrap()).collect();
            let advance: f64 = glyphs
                .iter()
                .map(|g| face.glyph_hor_advance(*g).unwrap() as f64)
                .sum();
            let cap = face.capital_height().unwrap() as f64;
            let scale = (0.78 / advance).min(0.135 / cap);
            let mut pen = -advance / 2.;
            for glyph in glyphs {
                let mut outline = Outline::default();
                face.outline_glyph(glyph, &mut outline);
                if !outline.0.is_empty() {
                    // These uppercase glyphs each have one exterior contour; any others are counters.
                    outline.0.sort_by(|a, b| area(b).total_cmp(&area(a)));
                    let mut points: Vec<f64> = Vec::new();
                    let mut holes = Vec::new();
                    for (i, contour) in outline.0.iter().enumerate() {
                        if i > 0 {
                            holes.push(points.len() / 2);
                        }
                        for p in contour {
                            points.extend(p);
                        }
                    }
                    let indices = earcutr::earcut(&points, &holes, 2).unwrap();
                    for t in indices.chunks_exact(3) {
                        let p = std::array::from_fn(|i| {
                            let x = ((pen + points[t[i] * 2]) * scale) as f32;
                            [
                                -0.25 - side * x,
                                y + side * LETTERING_STANDOFF,
                                bottom + (points[t[i] * 2 + 1] * scale) as f32,
                            ]
                        });
                        triangle(&mut bytes, p, side, [0.86, 0.025, 0.045]);
                    }
                }
                pen += face.glyph_hor_advance(glyph).unwrap() as f64;
            }
        }
    }
    let count = ((bytes.len() - 32) / 40) as u32;
    bytes[4..8].copy_from_slice(&count.to_le_bytes());
    fs::write(path, &bytes).unwrap();
    println!("Baked {count} vertices ({} bytes)", bytes.len());
}

#[cfg(test)]
mod tests {
    #[test]
    fn baked_lettering_is_raised_above_both_panels() {
        let bytes = include_bytes!("../../../data/landmarks/25.mesh");
        let vertices: Vec<[f32; 10]> = bytes[32..]
            .chunks_exact(40)
            .map(|v| {
                std::array::from_fn(|i| f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect();
        for side in [-1., 1.] {
            let face: Vec<_> = vertices
                .iter()
                .filter(|v| v[2] > 2.08 && v[4] * side > 0.99)
                .collect();
            let panel: Vec<_> = face
                .iter()
                .filter(|v| v[6..9].iter().all(|c| *c > 0.9))
                .collect();
            let letters: Vec<_> = face
                .iter()
                .filter(|v| v[6] > 0.6 && v[7] < 0.1 && v[8] < 0.1)
                .collect();
            assert_eq!(panel.len(), 6, "missing panel on side {side}");
            assert!(letters.len() > 300, "missing lettering on side {side}");
            let panel_depth = panel
                .iter()
                .map(|v| v[1] * side)
                .fold(f32::NEG_INFINITY, f32::max);
            for v in letters {
                assert!(
                    v[1] * side - panel_depth >= 0.019,
                    "lettering must remain visibly raised on side {side}"
                );
            }
        }
    }
}
