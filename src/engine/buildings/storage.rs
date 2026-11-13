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
        let source = opencity::expansion::edited_source(&self.source, &self.program, &after)?;
        self.write_source(after, source)
    }
    fn write_source(&mut self, after: Program, source: String) -> Result<(), String> {
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
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub(super) fn delete(&mut self, index: usize) -> Result<(), String> {
        let item = self.items.get(index).ok_or("No building selected")?;
        let name = item
            .feature
            .name
            .as_ref()
            .ok_or("Building has no identity")?;
        let removed: Vec<_> = self
            .program
            .statements
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                (i == item.statement
                    || matches!(s,Statement::Call(c) if c.name=="elevation"
                && c.args.first()==Some(&Argument::Identifier(name.clone()))))
                .then_some(i)
            })
            .collect();
        let (after, source) = remove_calls(&self.source, &self.program, &removed)?;
        self.write_source(after, source)?;
        self.items.remove(index);
        for item in &mut self.items {
            item.statement -= removed.iter().filter(|&&old| old < item.statement).count();
        }
        self.selected = None;
        self.cancel_interaction();
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
        let raw = crate::hermes::external_models::decode_shared(&bytes)?;
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

/// Remove only selected top-level calls, leaving all other source bytes alone.
fn remove_calls(
    source: &str,
    before: &Program,
    removed: &[usize],
) -> Result<(Program, String), String> {
    if opencity::parse(source).map_err(|e| e.to_string())? != *before {
        return Err("Map source changed".into());
    }
    let tokens = opencity::tokenize(source).map_err(|e| e.to_string())?;
    let mut ranges = Vec::new();
    for &index in removed {
        let old = &before.statements[index];
        let Statement::Call(call) = old else {
            return Err("Cannot delete a definition".into());
        };
        let name = call.assigns.first().unwrap_or(&call.name);
        let mut found = None;
        for (i, token) in tokens.iter().enumerate().filter(|(_, t)| t.text == *name) {
            let Some(end) = tokens[i..]
                .iter()
                .find(|t| t.kind == opencity::TokenKind::Semicolon)
            else {
                continue;
            };
            let range = token.span.offset..end.span.offset + 1;
            if opencity::parse(&source[..range.start])
                .ok()
                .is_some_and(|p| p.statements.len() == index)
                && opencity::parse(&source[range.clone()])
                    .ok()
                    .is_some_and(|p| p.statements == vec![old.clone()])
            {
                found = Some(range);
                break;
            }
        }
        ranges.push(found.ok_or("Could not locate the building source call")?);
    }
    ranges.sort_by_key(|r| r.start);
    let mut output = source.to_owned();
    for range in ranges.into_iter().rev() {
        output.replace_range(range, "");
    }
    let after = Program {
        statements: before
            .statements
            .iter()
            .enumerate()
            .filter(|(i, _)| !removed.contains(i))
            .map(|(_, s)| s.clone())
            .collect(),
    };
    if opencity::parse(&output).map_err(|e| e.to_string())? != after {
        return Err("Building deletion source validation failed".into());
    }
    Ok((after, output))
}
