//! Versioned, compressed CPU game data. GPU resources are recreated per device.
#[cfg(not(target_arch = "wasm32"))]
mod cache;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
use super::TEXTURE_SIZE;
#[cfg(not(target_arch = "wasm32"))]
pub use cache::maintain;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
use flate2::read::GzDecoder;
#[cfg(not(target_arch = "wasm32"))]
use flate2::{write::GzEncoder, Compression};
use opencity::runtime::Mesh;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
use opencity::runtime::Vertex;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
use std::io::{self, Read};
#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
const MAGIC: &[u8; 16] = b"MTL-GAME-DATA002";
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
const MAX_VERTICES: usize = 20_000_000;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
const ATLAS_BYTES: usize = TEXTURE_SIZE as usize * TEXTURE_SIZE as usize * 4;
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey(u64);
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
impl CacheKey {
    pub fn for_map(source: &[u8], elevation: &[u8]) -> Self {
        Self::from_parts(&[
            env!("MTL_JOBINES_GENERATOR_KEY").as_bytes(),
            include_bytes!("../../assets/default-housing.road"),
            source,
            elevation,
        ])
    }
    fn from_parts(parts: &[&[u8]]) -> Self {
        let mut value = 0xcbf29ce484222325_u64;
        for bytes in parts {
            for byte in (bytes.len() as u64)
                .to_le_bytes()
                .iter()
                .chain(bytes.iter())
            {
                value = (value ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
        }
        Self(value)
    }
}
pub struct GameData {
    pub world: Mesh,
    pub details: Mesh,
    pub atlas: Vec<u8>,
}
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
fn read_u64(reader: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}
#[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
fn read_mesh(reader: &mut impl Read, budget: usize, retain: bool) -> io::Result<(Mesh, usize)> {
    let count = usize::try_from(read_u64(reader)?).map_err(|_| invalid("vertex count overflow"))?;
    if count > budget || count % 3 != 0 {
        return Err(invalid("invalid vertex count"));
    }
    let mut mesh = Mesh::default();
    // Grow with validated input instead of allocating an untrusted count up front.
    let mut bytes = vec![0_u8; 1024 * 52];
    for start in (0..count).step_by(1024) {
        let size = (count - start).min(1024) * 52;
        reader.read_exact(&mut bytes[..size])?;
        for vertex in bytes[..size].chunks_exact(52) {
            let values: [f32; 13] = std::array::from_fn(|i| {
                f32::from_le_bytes(vertex[i * 4..i * 4 + 4].try_into().unwrap())
            });
            if values.iter().any(|value| !value.is_finite()) {
                return Err(invalid("nonfinite cached vertex"));
            }
            if retain {
                mesh.vertices.push(Vertex {
                    position: values[0..3].try_into().unwrap(),
                    normal: values[3..6].try_into().unwrap(),
                    color: values[6..9].try_into().unwrap(),
                    surface: values[9..13].try_into().unwrap(),
                });
            }
        }
    }
    Ok((mesh, count))
}
impl GameData {
    #[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
    pub fn decode(input: impl Read, expected: CacheKey) -> io::Result<Self> {
        Self::decode_geometry(input, expected, true)
    }
    /// Validate old full-map caches while retaining only the minimap atlas.
    /// Render tiles are regenerated from the compiled city on demand.
    #[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
    pub fn decode_for_streaming(input: impl Read, expected: CacheKey) -> io::Result<Self> {
        Self::decode_geometry(input, expected, false)
    }
    #[cfg(any(not(target_arch = "wasm32"), bundled_game_data, test))]
    fn decode_geometry(input: impl Read, expected: CacheKey, retain: bool) -> io::Result<Self> {
        let mut reader = GzDecoder::new(input);
        let mut magic = [0; 16];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(invalid("unsupported game data version"));
        }
        if read_u64(&mut reader)? != expected.0 {
            return Err(invalid(
                "game data belongs to another map or generator version",
            ));
        }
        let (world, count) = read_mesh(&mut reader, MAX_VERTICES, retain)?;
        let (details, _) = read_mesh(&mut reader, MAX_VERTICES - count, retain)?;
        if read_u64(&mut reader)? != ATLAS_BYTES as u64 {
            return Err(invalid("invalid atlas size"));
        }
        let mut atlas = vec![0; ATLAS_BYTES];
        reader.read_exact(&mut atlas)?;
        let mut trailing = [0];
        if reader.read(&mut trailing)? != 0 {
            return Err(invalid("unexpected trailing game data"));
        }
        Ok(Self {
            world,
            details,
            atlas,
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_for_streaming(path: &Path, expected: CacheKey) -> io::Result<Self> {
        Self::decode_for_streaming(BufReader::new(fs::File::open(path)?), expected)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(path: &Path, expected: CacheKey) -> io::Result<Self> {
        Self::decode(BufReader::new(fs::File::open(path)?), expected)
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn write_mesh(writer: &mut impl Write, mesh: &Mesh) -> io::Result<()> {
    if mesh.vertices.len() > MAX_VERTICES || mesh.vertices.len() % 3 != 0 {
        return Err(invalid(&format!(
            "invalid mesh size: {} vertices",
            mesh.vertices.len()
        )));
    }
    writer.write_all(&(mesh.vertices.len() as u64).to_le_bytes())?;
    let mut bytes = Vec::with_capacity(1024 * 52);
    for chunk in mesh.vertices.chunks(1024) {
        bytes.clear();
        for v in chunk {
            for value in v
                .position
                .iter()
                .chain(&v.normal)
                .chain(&v.color)
                .chain(&v.surface)
            {
                if !value.is_finite() {
                    return Err(invalid("nonfinite vertex"));
                }
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        writer.write_all(&bytes)?;
    }
    Ok(())
}
#[cfg(not(target_arch = "wasm32"))]
pub fn write(
    path: &Path,
    key: CacheKey,
    world: &Mesh,
    details: &Mesh,
    atlas: &[u8],
) -> io::Result<()> {
    if world.vertices.len().saturating_add(details.vertices.len()) > MAX_VERTICES {
        return Err(invalid("combined mesh size exceeds cache budget"));
    }
    if atlas.len() != ATLAS_BYTES {
        return Err(invalid("invalid atlas size"));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let file = fs::File::create(&temporary)?;
        let mut writer = GzEncoder::new(BufWriter::new(file), Compression::fast());
        writer.write_all(MAGIC)?;
        writer.write_all(&key.0.to_le_bytes())?;
        write_mesh(&mut writer, world)?;
        write_mesh(&mut writer, details)?;
        writer.write_all(&(atlas.len() as u64).to_le_bytes())?;
        writer.write_all(atlas)?;
        let mut file = writer.finish()?;
        file.flush()?;
        file.get_ref().sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
#[cfg(not(target_arch = "wasm32"))]
pub fn cache_path(key: CacheKey) -> PathBuf {
    let base = if let Some(path) = std::env::var_os("MTL_JOBINES_CACHE_DIR") {
        PathBuf::from(path)
    } else if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(path).join("mtl_jobines")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Caches/mtl_jobines")
    } else if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        PathBuf::from(path).join("mtl_jobines")
    } else {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache/mtl_jobines")
    };
    base.join(format!("{:016x}.game-data.gz", key.0))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    fn triangle() -> Mesh {
        let mut mesh = Mesh::default();
        mesh.triangle(
            [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            [0.1, 0.2, 0.3],
            opencity::runtime::SurfaceKind::Asphalt,
            [0., 0., 0.],
        );
        mesh
    }
    #[test]
    fn keys_change_with_map_elevation_and_generator() {
        let key = CacheKey::for_map(b"map", b"elevation");
        assert_ne!(key, CacheKey::for_map(b"other map", b"elevation"));
        assert_ne!(key, CacheKey::for_map(b"map", b"other elevation"));
        assert_ne!(
            CacheKey::from_parts(&[b"v1", b"map"]),
            CacheKey::from_parts(&[b"v2", b"map"])
        );
        assert_ne!(
            CacheKey::from_parts(&[b"ab", b"c"]),
            CacheKey::from_parts(&[b"a", b"bc"])
        );
    }
    #[test]
    fn atomic_cache_round_trip_rejects_stale_truncated_and_corrupt_data() {
        let dir = std::env::temp_dir().join(format!("mtl-game-data-test-{}", std::process::id()));
        let path = dir.join("map.gz");
        let key = CacheKey::for_map(b"test map", b"");
        let mesh = triangle();
        let mut atlas = vec![0; ATLAS_BYTES];
        atlas[0..4].copy_from_slice(&[1, 2, 3, 255]);
        let mut details = triangle();
        for vertex in &mut details.vertices {
            vertex.surface = [40., 0.25, 0.75, 8.];
        }
        let mut room = triangle();
        for vertex in &mut room.vertices {
            vertex.surface = [41., 0.4, 0.6, 2.];
        }
        details.vertices.extend(room.vertices);
        write(&path, key, &mesh, &details, &atlas).unwrap();

        let data = GameData::read(&path, key).unwrap();
        assert_eq!(data.world.vertices.len(), 3);
        for (actual, expected) in data.world.vertices.iter().zip(&mesh.vertices) {
            assert_eq!(actual.position, expected.position);
            assert_eq!(actual.normal, expected.normal);
            assert_eq!(actual.color, expected.color);
            assert_eq!(actual.surface, expected.surface);
        }
        assert_eq!(data.details.vertices.len(), details.vertices.len());
        for (a, b) in data.details.vertices.iter().zip(&details.vertices) {
            assert_eq!(a.surface, b.surface);
            assert_eq!(a.position, b.position);
        }

        assert_eq!(data.atlas, atlas);
        let streamed = GameData::read_for_streaming(&path, key).unwrap();
        assert!(streamed.world.vertices.is_empty() && streamed.details.vertices.is_empty());
        assert_eq!(streamed.atlas, atlas);
        drop(data);
        assert!(GameData::read(&path, CacheKey::for_map(b"changed", b"")).is_err());
        let mut bytes = fs::read(&path).unwrap();
        assert!(GameData::decode(&bytes[..bytes.len() - 8], key).is_err());
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(GameData::decode(bytes.as_slice(), key).is_err());
        assert!(GameData::decode(b"not game data".as_slice(), key).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn oversized_mesh_count_is_rejected_before_allocation() {
        let key = CacheKey::for_map(b"x", b"");
        let mut writer = GzEncoder::new(Vec::new(), Compression::fast());
        writer.write_all(MAGIC).unwrap();
        writer.write_all(&key.0.to_le_bytes()).unwrap();
        writer.write_all(&u64::MAX.to_le_bytes()).unwrap();
        assert!(GameData::decode(writer.finish().unwrap().as_slice(), key).is_err());
    }
}
