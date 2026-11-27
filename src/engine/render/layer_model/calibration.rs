use super::{Model, CALIBRATION_LEVELS, CALIBRATION_PROBES, PERIOD, PHASES, TEXELS, TILE};

fn tile_index(x: i32, y: i32) -> usize {
    y.rem_euclid(TILE as i32) as usize * TILE + x.rem_euclid(TILE as i32) as usize
}

fn neighbor(index: usize, [x, y, _, _]: [i32; 4]) -> usize {
    tile_index((index % TILE) as i32 + x, (index / TILE) as i32 + y)
}

fn axis_weight(position: usize) -> f32 {
    let sine = ((position % PERIOD) as f32 + 0.5) * (std::f32::consts::PI / TILE as f32);
    let weight = sine.sin().powi(2);
    if position < PERIOD {
        weight
    } else {
        1. - weight
    }
}

pub(super) fn flat_field_bias(model: &Model) -> Option<Vec<[f32; 4]>> {
    let settings = &model.settings;
    let [first_count, second_count, readout_count, _] = settings.counts.map(|count| count as usize);
    let windows: [_; TILE] = std::array::from_fn(axis_weight);
    let mut first = vec![0.; TEXELS];
    let mut second = vec![0.; TEXELS];
    let mut unblended = vec![[0.; 3]; TEXELS];
    let mut table = Vec::with_capacity(PERIOD * PERIOD * CALIBRATION_PROBES);

    for probe in 0..CALIBRATION_PROBES {
        let rgb = [
            probe / CALIBRATION_LEVELS.pow(2),
            probe / CALIBRATION_LEVELS % CALIBRATION_LEVELS,
            probe % CALIBRATION_LEVELS,
        ]
        .map(|component| component as f32 / (CALIBRATION_LEVELS - 1) as f32);
        let luminance = rgb
            .iter()
            .zip(&settings.luminance)
            .map(|(code, coefficient)| code * coefficient)
            .sum::<f32>();
        for index in 0..TEXELS {
            if first_count == 0 {
                first[index] = luminance;
            } else {
                let mut value = settings.biases[0];
                for (channel, code) in rgb.iter().enumerate() {
                    for tap in 0..first_count {
                        value += code * model.weights[channel * TEXELS + index][tap];
                    }
                }
                first[index] = value.max(0.);
            }
        }
        for index in 0..TEXELS {
            if second_count == 0 {
                second[index] = first[index];
            } else {
                let mut value = settings.biases[1];
                for tap in 0..second_count {
                    value += first[neighbor(index, settings.hidden2[tap])]
                        * model.weights[3 * TEXELS + index][tap];
                }
                second[index] = value.max(0.)
                    + if settings.skips[0] != 0 {
                        luminance
                    } else {
                        0.
                    };
            }
        }
        // The output bias and fixed input skips are constant on a flat field.
        // They cancel when subtracting the spatial mean, so omit them here.
        for (index, pixel) in unblended.iter_mut().enumerate() {
            for (channel, output) in pixel.iter_mut().enumerate() {
                *output = (0..readout_count)
                    .map(|tap| {
                        second[neighbor(index, settings.readout[tap])]
                            * model.weights[(4 + channel) * TEXELS + index][tap]
                    })
                    .sum();
            }
        }

        let start = table.len();
        let mut sum = [0.0_f64; 3];
        for y in 0..PERIOD {
            for x in 0..PERIOD {
                let mut pixel = [0.; 4];
                for [dx, dy] in PHASES {
                    let x = x + dx as usize;
                    let y = y + dy as usize;
                    let fade = windows[x] * windows[y];
                    for (channel, value) in pixel[..3].iter_mut().enumerate() {
                        *value += fade * unblended[y * TILE + x][channel];
                    }
                }
                for (channel, total) in sum.iter_mut().enumerate() {
                    *total += f64::from(pixel[channel]);
                }
                table.push(pixel);
            }
        }
        let mean = sum.map(|value| (value / (PERIOD * PERIOD) as f64) as f32);
        for pixel in &mut table[start..] {
            for (channel, value) in pixel[..3].iter_mut().enumerate() {
                *value -= mean[channel];
                if !value.is_finite() {
                    return None;
                }
            }
        }
    }
    Some(table)
}
