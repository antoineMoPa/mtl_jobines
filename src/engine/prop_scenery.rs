//! Cache intact prop vertices until their exact visible selection changes.
use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::DynamicWorld;
use opencity::runtime::Mesh;
use std::collections::HashMap;

struct CachedBatch {
    ids: Vec<usize>,
    mesh: GpuMesh,
    last_seen: u64,
}
#[derive(Default)]
pub struct PropScenery {
    batches: HashMap<(i32, i32), CachedBatch>,
    visible: Vec<(i32, i32)>,
    scratch: Mesh,
    frame: u64,
}
impl PropScenery {
    pub fn update(&mut self, renderer: &Renderer, world: &DynamicWorld) {
        self.frame += 1;
        self.visible.clear();
        for batch in world.visible_prop_batches() {
            self.visible.push(batch.key);
            if let Some(cached) = self.batches.get_mut(&batch.key) {
                cached.last_seen = self.frame;
                if cached.ids == batch.ids {
                    continue;
                }
            }
            world.write_prop_batch(&batch, &mut self.scratch);
            self.batches.insert(
                batch.key,
                CachedBatch {
                    ids: batch.ids,
                    mesh: renderer.upload(&self.scratch),
                    last_seen: self.frame,
                },
            );
        }
        // Keep a short reuse window without accumulating the entire city's GPU
        // prop geometry as the player travels. Visibility never uses stale IDs.
        self.batches
            .retain(|_, batch| self.frame - batch.last_seen < 240);
    }
    pub fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.visible
            .iter()
            .filter_map(|key| self.batches.get(key).map(|batch| &batch.mesh))
    }
}
