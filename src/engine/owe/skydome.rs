//! Resolve package names once the renderer has prepared their GPU resources.
use super::world::{NOT_FOUND, OK};
use crate::engine::render::Renderer;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Assets {
    pub(super) textures: HashMap<String, Vec<u8>>,
    pub(super) shaders: HashMap<String, String>,
}

#[derive(Default)]
pub(super) struct Skydome {
    pub textures: Selection,
    pub shaders: Selection,
}

impl Skydome {
    pub fn apply(&self, renderer: &mut Renderer) {
        renderer.sky.texture_index = self.textures.selected;
        renderer.sky.shader_index = self.shaders.selected;
    }
}

#[derive(Default)]
pub(super) struct Selection {
    names: HashMap<String, usize>,
    selected: usize,
}

impl Selection {
    pub fn select(&mut self, name: &str) -> i32 {
        self.selected = if name.is_empty() {
            0
        } else if let Some(&index) = self.names.get(name) {
            index
        } else {
            return NOT_FOUND;
        };
        OK
    }
}

impl Assets {
    pub async fn prepare(self, renderer: &mut Renderer) -> Result<Skydome, String> {
        let mut sky = Skydome::default();
        for name in self.textures.keys().chain(self.shaders.keys()) {
            if name.is_empty() || name.len() > 159 || name.contains('\0') {
                return Err(format!("invalid skydome asset name {name:?}"));
            }
        }
        for (name, png) in self.textures {
            let index = renderer
                .sky
                .add_texture(&renderer.device, &renderer.queue, &png)
                .map_err(|e| format!("skydome texture {name:?}: {e}"))?;
            sky.textures.names.insert(name, index);
        }
        for (name, source) in self.shaders {
            let index = renderer
                .sky
                .add_shader(&renderer.device, &source)
                .await
                .map_err(|e| format!("skydome shader {name:?}: {e}"))?;
            sky.shaders.names.insert(name, index);
        }
        Ok(sky)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::engine::owe::world::{Packet, World};
    use bytemuck::Zeroable;
    use glam::Vec3;

    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn client_skydome_switches_and_resets_each_resource() {
        let mut png = vec![];
        {
            let mut encoder = png::Encoder::new(&mut png, 1, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[255, 0, 0])
                .unwrap();
        }
        let pack = serde_json::json!({
            "title": "Sky test", "maps": {}, "prefabs": {},
            "skydome": {
                "textures": {"red": png},
                "shaders": {"swap": "fn owe_skydome(s: SkydomeInput) -> vec3<f32> { return s.panorama.brg; }"}
            }
        });
        let mut world = World::new(&pack.to_string()).unwrap();
        let mut renderer = pollster::block_on(Renderer::with_catalog(None, 64, 64, false)).unwrap();
        world.skydome =
            pollster::block_on(std::mem::take(&mut world.package.skydome).prepare(&mut renderer))
                .unwrap();

        fn pixel(r: &Renderer) -> [u8; 3] {
            let png = r
                .capture_png(|view| {
                    r.render_scene(view, &[], Vec3::ZERO, Vec3::new(0., 1., 0.5), 0., None);
                })
                .unwrap();
            let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
            let mut image = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut image).unwrap();
            image[(16 * 64 + 32) * 4..][..3].try_into().unwrap()
        }

        let default = pixel(&renderer);
        let mut select = |op, name: &str, expected| {
            let mut packet = Packet {
                op,
                ..Packet::zeroed()
            };
            packet.text[..name.len()].copy_from_slice(name.as_bytes());
            assert_eq!(world.call(&mut packet), expected);
            world.skydome.apply(&mut renderer);
            pixel(&renderer)
        };
        assert_eq!(select(32, "red", OK), [255, 0, 0]);
        assert_eq!(select(33, "swap", OK), [0, 255, 0]);
        assert_eq!(select(33, "missing", NOT_FOUND), [0, 255, 0]);
        assert_ne!(select(32, "", OK), default);
        assert_eq!(select(32, "red", OK), [0, 255, 0]);
        assert_eq!(select(33, "", OK), [255, 0, 0]);
        assert_eq!(select(32, "", OK), default);

        let bad = Assets {
            shaders: HashMap::from([("broken".into(), "bad wgsl".into())]),
            ..Default::default()
        };
        assert!(pollster::block_on(bad.prepare(&mut renderer))
            .err()
            .unwrap()
            .contains("broken"));
    }
}
