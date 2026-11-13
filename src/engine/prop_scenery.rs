//! Cache intact scenery until its exact visible selection or signal phase changes.
use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::{DynamicWorld, PropBatch, SignalBatch};
use opencity::runtime::Mesh;
use std::collections::HashMap;

struct CachedBatch {
    mesh: GpuMesh,
    last_seen: u64,
}
struct CachedSignal {
    selection: SignalBatch,
    mesh: GpuMesh,
    last_seen: u64,
}
#[derive(Default)]
pub struct PropScenery {
    batches: HashMap<usize, CachedBatch>,
    visible: Vec<usize>,
    signals: HashMap<usize, CachedSignal>,
    visible_signals: Vec<usize>,
    scratch: Mesh,
    frame: u64,
}
impl PropScenery {
    pub fn update(&mut self, renderer: &Renderer, world: &DynamicWorld) {
        self.frame += 1;
        self.visible.clear();
        self.visible_signals.clear();
        for batch in world.visible_prop_batches() {
            for id in batch.ids {
                self.visible.push(id);
                if let Some(cached) = self.batches.get_mut(&id) {
                    cached.last_seen = self.frame;
                    continue;
                }
                // Keep exact physics visibility, but cache each immutable prop once.
                // A neighbour entering a render tile must not rebuild its LOD meshes.
                world.write_prop_batch(
                    &PropBatch {
                        key: batch.key,
                        ids: vec![id],
                    },
                    &mut self.scratch,
                );
                self.batches.insert(
                    id,
                    CachedBatch {
                        mesh: renderer.upload_scenery(&self.scratch),
                        last_seen: self.frame,
                    },
                );
            }
        }
        for selection in world.standing_signal_batches() {
            self.visible_signals.push(selection.id);
            if let Some(cached) = self.signals.get_mut(&selection.id) {
                cached.last_seen = self.frame;
                if cached.selection == selection {
                    continue;
                }
            }
            world.write_signal_batch(&selection, &mut self.scratch);
            self.signals.insert(
                selection.id,
                CachedSignal {
                    selection,
                    // Phases change during play; retain exact signal geometry and
                    // avoid rebuilding LOD variants at a light transition.
                    mesh: renderer.upload(&self.scratch),
                    last_seen: self.frame,
                },
            );
        }
        // Keep a short reuse window without accumulating the entire city's GPU
        // prop geometry as the player travels. Visibility never uses stale IDs.
        self.batches
            .retain(|_, batch| self.frame - batch.last_seen < 240);
        self.signals
            .retain(|_, batch| self.frame - batch.last_seen < 240);
    }
    pub fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.visible
            .iter()
            .filter_map(|key| self.batches.get(key).map(|batch| &batch.mesh))
            .chain(
                self.visible_signals
                    .iter()
                    .filter_map(|id| self.signals.get(id).map(|batch| &batch.mesh)),
            )
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
