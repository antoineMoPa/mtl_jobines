//! Freehand marks on the captured pixels; UI points are mapped to image pixels once.
use egui::{ColorImage, Pos2, Rect, Sense, TextureHandle, TextureOptions};

pub(super) struct Annotation {
    size: [usize; 2],
    pixels: Vec<u8>,
    texture: Option<TextureHandle>,
    previous: Option<Pos2>,
    dirty: bool,
}
impl Annotation {
    pub fn from_png(bytes: &[u8]) -> Result<Self, String> {
        let mut decoder = png::Decoder::new(bytes);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
        let size = [info.width as usize, info.height as usize];
        let pixels = match info.color_type {
            png::ColorType::Rgba => buffer[..info.buffer_size()].to_vec(),
            png::ColorType::Rgb => buffer[..info.buffer_size()]
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => buffer[..info.buffer_size()]
                .iter()
                .flat_map(|&p| [p, p, p, 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => buffer[..info.buffer_size()]
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            _ => return Err("Unsupported screenshot pixel format".into()),
        };
        Ok(Self {
            size,
            pixels,
            texture: None,
            previous: None,
            dirty: false,
        })
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        if self.texture.is_none() {
            self.texture = Some(ctx.load_texture(
                "report screenshot",
                ColorImage::from_rgba_unmultiplied(self.size, &self.pixels),
                TextureOptions::LINEAR,
            ));
        }
        let bounds = ctx.content_rect();
        egui::Area::new(egui::Id::new("report-screenshot"))
            .order(egui::Order::Background)
            .fixed_pos(bounds.min)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                let (rect, response) =
                    ui.allocate_exact_size(bounds.size(), Sense::click_and_drag());
                ui.painter().image(
                    self.texture.as_ref().unwrap().id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                    egui::Color32::WHITE,
                );
                let pointer = ctx.input(|i| i.pointer.interact_pos());
                let pressed = ctx.input(|i| i.pointer.primary_pressed());
                let down = ctx.input(|i| i.pointer.primary_down());
                if response.hovered() && pressed {
                    self.previous = pointer;
                }
                if let (Some(previous), Some(current)) = (self.previous, pointer) {
                    if response.hovered() && (down || ctx.input(|i| i.pointer.primary_released())) {
                        let to_pixel = |p: Pos2| {
                            egui::pos2(
                                (p.x - rect.left()) / rect.width() * self.size[0] as f32,
                                (p.y - rect.top()) / rect.height() * self.size[1] as f32,
                            )
                        };
                        let radius = (2. * self.size[0] as f32 / rect.width()).max(1.);
                        let changed = self.stroke(to_pixel(previous), to_pixel(current), radius);
                        if let Some((origin, size, pixels)) = changed {
                            self.texture.as_mut().unwrap().set_partial(
                                origin,
                                ColorImage::from_rgba_unmultiplied(size, &pixels),
                                TextureOptions::LINEAR,
                            );
                        }
                        self.previous = Some(current);
                        ctx.request_repaint();
                    } else {
                        self.previous = None;
                    }
                }
                if !down {
                    self.previous = None;
                }
            });
    }
    fn stroke(
        &mut self,
        a: Pos2,
        b: Pos2,
        radius: f32,
    ) -> Option<([usize; 2], [usize; 2], Vec<u8>)> {
        let low = [
            (a.x.min(b.x) - radius - 1.).floor().max(0.) as usize,
            (a.y.min(b.y) - radius - 1.).floor().max(0.) as usize,
        ];
        let high = [
            ((a.x.max(b.x) + radius + 1.).ceil().max(0.) as usize).min(self.size[0]),
            ((a.y.max(b.y) + radius + 1.).ceil().max(0.) as usize).min(self.size[1]),
        ];
        if low[0] >= high[0] || low[1] >= high[1] {
            return None;
        }
        let d = b - a;
        let length = d.length_sq();
        for y in low[1]..high[1] {
            for x in low[0]..high[0] {
                let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                let t = if length > 1e-8 {
                    ((p - a).dot(d) / length).clamp(0., 1.)
                } else {
                    0.
                };
                let distance = p.distance(a + d * t);
                let coverage = (radius + 0.5 - distance).clamp(0., 1.);
                if coverage <= 0. {
                    continue;
                }
                let offset = (y * self.size[0] + x) * 4;
                for (i, red) in [255., 24., 32.].into_iter().enumerate() {
                    self.pixels[offset + i] = (self.pixels[offset + i] as f32 * (1. - coverage)
                        + red * coverage)
                        .round() as u8;
                }
                self.dirty = true;
            }
        }
        let size = [high[0] - low[0], high[1] - low[1]];
        let mut pixels = Vec::with_capacity(size[0] * size[1] * 4);
        for y in low[1]..high[1] {
            pixels.extend_from_slice(
                &self.pixels[(y * self.size[0] + low[0]) * 4..(y * self.size[0] + high[0]) * 4],
            );
        }
        Some((low, size, pixels))
    }
    pub fn screenshot(&self, original: &[u8]) -> Result<Vec<u8>, String> {
        if !self.dirty {
            return Ok(original.to_vec());
        }
        let mut result = Vec::new();
        {
            let mut encoder =
                png::Encoder::new(&mut result, self.size[0] as u32, self.size[1] as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .map_err(|e| e.to_string())?
                .write_image_data(&self.pixels)
                .map_err(|e| e.to_string())?;
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests;
