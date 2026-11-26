//! Raise only local piste depressions that obstruct a gentle route to either base.
//! No piste cells are edited. PNG pixels outside the hill and terminal/road buffers stay intact.
use opencity::heightmap::{ElevationMap, ElevationMapSpec};
use std::{cmp::Ordering, collections::BinaryHeap, fs, io::Cursor, path::Path};
const N: usize = 1024;
const GRADE: f32 = 0.003; // 0.3% is deliberately gentle; no wholesale steepening.
#[derive(Clone, Copy)]
struct Entry {
    height: f32,
    index: usize,
}
impl PartialEq for Entry {
    fn eq(&self, o: &Self) -> bool {
        self.height == o.height && self.index == o.index
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Entry {
    fn cmp(&self, o: &Self) -> Ordering {
        o.height
            .total_cmp(&self.height)
            .then_with(|| o.index.cmp(&self.index))
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let apply = std::env::args().any(|a| a == "--apply");
    let check_only = std::env::args().any(|a| a == "--check");
    let target_grade = if check_only { 0.0025 } else { GRADE };
    let path = Path::new("data/maps/mtl/elevation.png");
    let bytes = fs::read(path)?;
    let source = fs::read_to_string("data/maps/mtl/map.road")?;
    let program = opencity::parse(&source).map_err(|e| e.to_string())?;
    let spec = ElevationMapSpec::from_program(&program)?.ok_or("missing elevation map")?;
    let map = ElevationMap::from_png(spec.clone(), &bytes)?;
    let cells =
        mtl_world::hermes::ski_hill::read_area_png(&fs::read("data/maps/mtl/ski-hill-areas.png")?)?;
    let piste = |p: [f32; 2]| {
        let c = ((p[0] + 10.) * 10.).floor() as isize;
        let r = ((-76. - p[1]) * 10.).floor() as isize;
        c >= 0 && c < 180 && r >= 0 && r < 190 && cells[r as usize * 180 + c as usize] == 2
    };
    let bases = [[3.8, -82.6], [-4.85, -92.05]];
    let protected = |p: [f32; 2]| {
        bases
            .iter()
            .any(|b| ((p[0] - b[0]).powi(2) + (p[1] - b[1]).powi(2)).sqrt() < 0.35)
            || (p[0] >= 3.1 && p[0] <= 8. && p[1] >= -82.35 && p[1] <= -81.65)
    };
    let mut original = Vec::with_capacity(N * N);
    let mut selected = vec![false; N * N];
    let mut target = vec![f32::INFINITY; N * N];
    let mut queue = BinaryHeap::new();
    let mut parent = vec![usize::MAX; N * N];
    for y in 0..N {
        for x in 0..N {
            let i = y * N + x;
            let p = map.point(x, y);
            original.push(p[2] * 100.);
            if piste([p[0], p[1]]) {
                selected[i] = true;
                if protected([p[0], p[1]]) {
                    target[i] = original[i];
                    queue.push(Entry {
                        height: target[i],
                        index: i,
                    });
                }
            }
        }
    }
    let spacing = [spec.size[0] * 100. / 1023., spec.size[1] * 100. / 1023.];
    while let Some(Entry { height, index: i }) = queue.pop() {
        if height > target[i] {
            continue;
        }
        let x = i % N;
        let y = i / N;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let xx = x as isize + dx;
                let yy = y as isize + dy;
                if xx < 0 || yy < 0 || xx >= N as isize || yy >= N as isize {
                    continue;
                }
                let j = yy as usize * N + xx as usize;
                if !selected[j] {
                    continue;
                }
                let a = map.point(x, y);
                let b = map.point(xx as usize, yy as usize);
                if !piste([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]) {
                    continue;
                }
                let distance =
                    ((dx as f32 * spacing[0]).powi(2) + (dy as f32 * spacing[1]).powi(2)).sqrt();
                let candidate = original[j].max(height + target_grade * distance);
                if candidate < target[j] {
                    target[j] = candidate;
                    parent[j] = i;
                    queue.push(Entry {
                        height: candidate,
                        index: j,
                    });
                }
            }
        }
    }
    let disconnected = (0..N * N)
        .filter(|&i| selected[i] && !target[i].is_finite())
        .count();
    if disconnected > 0 {
        return Err(format!("{disconnected} native piste samples do not connect to either base; terrain was not modified").into());
    }
    let mut delta = vec![0_f32; N * N];
    let mut changed = Vec::new();
    for i in 0..N * N {
        if selected[i] && target[i] - original[i] > 0.01 {
            delta[i] = target[i] - original[i];
            changed.push(i);
        }
    }
    let max = changed.iter().map(|&i| delta[i]).fold(0_f32, f32::max);
    if max > 6. {
        return Err(format!(
            "local correction would exceed the 6 m limit ({max:.2} m); terrain was not modified"
        )
        .into());
    }
    // Feather only into neighboring forest/unmarked terrain. Piste values retain their solved grades.
    let hard = delta.clone();
    for &i in &changed {
        let x = i % N;
        let y = i / N;
        for dy in -3..=3 {
            for dx in -3..=3 {
                let xx = x as isize + dx;
                let yy = y as isize + dy;
                if xx < 0 || yy < 0 || xx >= N as isize || yy >= N as isize {
                    continue;
                }
                let distance =
                    ((dx as f32 * spacing[0]).powi(2) + (dy as f32 * spacing[1]).powi(2)).sqrt();
                if distance >= 40. {
                    continue;
                }
                let j = yy as usize * N + xx as usize;
                if selected[j] {
                    continue;
                }
                let p = map.point(xx as usize, yy as usize);
                if protected([p[0], p[1]]) {
                    continue;
                }
                let t = (1. - distance / 40.).clamp(0., 1.);
                let weight = t * t * (3. - 2. * t);
                delta[j] = delta[j].max(hard[i] * weight);
            }
        }
    }
    let total_changed = delta.iter().filter(|&&d| d > 0.001).count();
    let mean = changed.iter().map(|&i| hard[i]).sum::<f32>() / changed.len().max(1) as f32;
    let report = serde_json::json!({"method":"Minimum local fill on the native PNG piste graph, with 0.3% drainage paths to chalet/south terminal buffers. Original heights are preserved wherever already sufficient; surrounding terrain is feathered over 40 m.","applied":apply,"pistePixels":selected.iter().filter(|&&v|v).count(),"correctedPistePixels":changed.len(),"totalChangedPixels":total_changed,"maximumRaiseMetres":max,"meanRaiseOnCorrectedPisteMetres":mean,"minimumTargetGradePercent":target_grade*100.,"protectedTerminalRadiusMetres":35,"pisteMaskChanged":false,"loweredPixels":0});
    println!("{}", serde_json::to_string_pretty(&report)?);
    fs::create_dir_all("data/maps/mtl/ski-hill-generated")?;
    fs::write(
        if check_only {
            "data/maps/mtl/ski-hill-generated/ski-hill-grade-check.json"
        } else {
            "data/maps/mtl/ski-hill-generated/ski-hill-grade-correction.json"
        },
        serde_json::to_vec_pretty(&report)?,
    )?;
    if check_only && max > 0.02 {
        return Err(format!("terrain still needs up to {max:.3} m of grade correction").into());
    }
    if !apply {
        return Ok(());
    }
    let mut reader = png::Decoder::new(Cursor::new(&bytes)).read_info()?;
    let mut raw = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut raw)?;
    if info.bit_depth != png::BitDepth::Sixteen || info.color_type != png::ColorType::Grayscale {
        return Err("grading expects the existing 16-bit grayscale map".into());
    }
    let low = spec.heights[0] * 100.;
    let range = (spec.heights[1] - spec.heights[0]) * 100.;
    let mut changed_encoded = 0;
    for i in 0..N * N {
        if delta[i] <= 0.001 {
            continue;
        }
        let old = u16::from_be_bytes([raw[2 * i], raw[2 * i + 1]]);
        let height = original[i] + delta[i];
        let value = (((height - low) / range * 65535.).round().clamp(0., 65535.) as u16).max(old);
        if value != old {
            raw[2 * i..2 * i + 2].copy_from_slice(&value.to_be_bytes());
            changed_encoded += 1;
        }
    }
    let folder = Path::new(".moontasks/ski-hill-design-6ff9d97a-df91-4499-999c-2dd3facd833e");
    let backup = folder.join("elevation-before.png");
    if !backup.exists() {
        fs::write(&backup, &bytes)?;
    }
    let mut encoded = Vec::new();
    {
        let mut writer = png::Encoder::new(&mut encoded, N as u32, N as u32);
        writer.set_color(png::ColorType::Grayscale);
        writer.set_depth(png::BitDepth::Sixteen);
        writer.write_header()?.write_image_data(&raw)?;
    }
    let check = ElevationMap::from_png(spec, &encoded)?;
    let mut bad = 0;
    for i in 0..N * N {
        if parent[i] == usize::MAX {
            continue;
        }
        let p = check.point(i % N, i / N);
        let j = parent[i];
        let q = check.point(j % N, j / N);
        if (p[2] - q[2]) * 100. < 0.02 {
            bad += 1;
        }
    }
    if bad > 0 {
        return Err(format!("{bad} solved native drainage links failed after PNG quantization; terrain was not modified").into());
    }
    fs::write(path, encoded)?;
    println!("Applied {changed_encoded} changed native PNG samples; drainage verification passed. Backup: {}",backup.display());
    Ok(())
}
