use super::*;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn number(n: f32) -> Argument {
    Argument::Number(format!("{n:.7}"))
}
pub(super) fn call(f: &LandscapeFeature) -> Result<Call, String> {
    let style = f.building_style.ok_or("Building has no style")?;
    let mut args = vec![number(f.height.ok_or("Building has no height")?)];
    if let Some(model) = &f.model {
        args.push(Argument::String(format!("\"{model}\"")));
    }
    let mut props = vec![
        (
            "material".into(),
            Argument::Identifier(style.material.keyword().into()),
        ),
        ("floor_height".into(), number(style.floor_height)),
        (
            "use".into(),
            Argument::Identifier(f.building_use.keyword().into()),
        ),
    ];
    if !style.collidable {
        props.push(("collision".into(), Argument::Identifier("none".into())));
    }
    if let Some(edge) = f.front_edge {
        props.push(("front_edge".into(), Argument::Number(edge.to_string())));
    }
    props.extend(style.architecture.options());
    args.push(Argument::Object(props));
    args.extend(f.points.iter().flatten().copied().map(number));
    Ok(Call {
        assigns: vec![f.name.clone().ok_or("Building has no identity")?],
        name: "building".into(),
        args,
    })
}
pub(super) fn set_height(program: &mut Program, feature: &LandscapeFeature) {
    if let Some(name) = &feature.name {
        opencity::set_elevation(program, name, feature.elevation);
    }
}
pub(super) fn read_model(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
        return Err("GLB exceeds 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("GLB exceeds 64 MiB".into());
    }
    Ok(bytes)
}
/// Content-addressed copy: never move the original and never overwrite another model.
fn copy_asset(base: &Path, bytes: &[u8]) -> Result<(String, bool), String> {
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    std::fs::create_dir_all(base.join("models")).map_err(|e| e.to_string())?;
    for suffix in 0..100 {
        let tail = if suffix == 0 {
            String::new()
        } else {
            format!("-{suffix}")
        };
        let relative = format!("models/building-{hash:016x}{tail}.glb");
        let path = base.join(&relative);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(error.to_string());
                }
                return Ok((relative, true));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if read_model(&path).is_ok_and(|old| old == bytes) {
                    return Ok((relative, false));
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Could not allocate a model asset name".into())
}
impl Document {
    pub(super) fn save(&mut self, after: Program) -> Result<(), String> {
        let unchanged = || {
            std::fs::read_to_string(&self.path)
                .map_err(|e| e.to_string())
                .and_then(|source| {
                    if source == self.source {
                        Ok(())
                    } else {
                        Err("Map changed on disk; reopen Buildings".into())
                    }
                })
        };
        unchanged()?;
        let source = opencity::expansion::edited_source(&self.source, &self.program, &after)?;
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp = self
            .path
            .with_extension(format!("road.building-{}-{id}.tmp", std::process::id()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| e.to_string())?;
            file.write_all(source.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            drop(file);
            unchanged()?;
            std::fs::rename(&temp, &self.path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result?;
        self.source = source;
        self.program = after;
        Ok(())
    }
    pub(super) fn import_model(&mut self, city: &City, path: &Path) -> Result<(), String> {
        let index = self.selected.ok_or("No building selected")?;
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
        {
            return Err("Choose a .glb file".into());
        }
        let bytes = read_model(path)?;
        let raw = Arc::new(crate::hermes::external_models::decode(&bytes)?);
        let (lo, hi) = crate::hermes::external_models::bounds(&raw);
        let size = hi - lo;
        let mut feature = self.items[index].feature.clone();
        let a = Vec2::from_array(feature.points[0]);
        let b = Vec2::from_array(feature.points[1]);
        let c = Vec2::from_array(feature.points[2]);
        feature.height = Some(size.z * ((b - a).length() / size.x).min((c - b).length() / size.y));
        self.validate(city, &feature, Some(index))?;
        let base = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let (relative, created) = copy_asset(&base, &bytes)?;
        feature.model = Some(relative.clone());
        let old = self.items[index].model.replace(raw);
        let result = self.edit(city, index, feature);
        if result.is_err() {
            self.items[index].model = old;
            if created {
                let _ = std::fs::remove_file(base.join(&relative));
            }
        }
        result
    }
    pub fn dropped_file(&mut self, city: &City, path: &Path) {
        if self.open {
            self.error = self.import_model(city, path).err().unwrap_or_default();
        }
    }
}
