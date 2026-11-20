//! Small content-keyed cache shared by map reload and the building tool.
use super::*;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock},
};
const BUDGET: usize = 16 * 1024 * 1024;
struct Entry {
    bytes: Arc<[u8]>,
    mesh: Arc<Mesh>,
    cost: usize,
}
#[derive(Default)]
struct Cache {
    entries: VecDeque<Entry>,
    cost: usize,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
pub(super) fn decode(bytes: &[u8]) -> Result<Arc<Mesh>, String> {
    let cache = CACHE.get_or_init(|| Mutex::new(Cache::default()));
    {
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = c.entries.iter().position(|e| e.bytes.as_ref() == bytes) {
            let entry = c.entries.remove(index).unwrap();
            let result = entry.mesh.clone();
            c.entries.push_back(entry);
            return Ok(result);
        }
    }
    let mesh = Arc::new(super::decode_model(bytes, ModelUse::Building)?);
    let cost = bytes.len() + mesh.vertices.len() * std::mem::size_of::<Vertex>();
    if cost <= BUDGET {
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = c.entries.iter().find(|e| e.bytes.as_ref() == bytes) {
            return Ok(entry.mesh.clone());
        }
        while c.cost + cost > BUDGET {
            if let Some(entry) = c.entries.pop_front() {
                c.cost -= entry.cost;
            }
        }
        c.entries.push_back(Entry {
            bytes: Arc::from(bytes),
            mesh: mesh.clone(),
            cost,
        });
        c.cost += cost;
    }
    Ok(mesh)
}
