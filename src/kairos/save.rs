use super::PlayerProgress;

pub(super) fn decode(bytes: &[u8]) -> Result<PlayerProgress, String> {
    if bytes.len() > 64 * 1024 {
        return Err("save exceeds 64 KiB".into());
    }
    let progress: PlayerProgress = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if progress.version != 1 {
        return Err(format!("unsupported save version {}", progress.version));
    }
    Ok(progress)
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or("home folder unavailable")?;
    Ok(std::path::PathBuf::from(home).join(".mtl_jobines/save.json"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn read() -> Result<Option<PlayerProgress>, String> {
    let path = path()?;
    match std::fs::read(path) {
        Ok(bytes) => decode(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write(progress: &PlayerProgress) -> Result<(), String> {
    use std::io::Write;
    let path = path()?;
    let parent = path.parent().ok_or("save folder unavailable")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".save.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec(progress).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("browser window unavailable")?
        .local_storage()
        .map_err(|_| "local storage access denied")?
        .ok_or_else(|| "local storage unavailable".into())
}

#[cfg(target_arch = "wasm32")]
pub(super) fn read() -> Result<Option<PlayerProgress>, String> {
    let value = storage()?
        .get_item("mtl_jobines.save.v1")
        .map_err(|_| "could not read local storage")?;
    value.map(|json| decode(json.as_bytes())).transpose()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn write(progress: &PlayerProgress) -> Result<(), String> {
    let json = serde_json::to_string(progress).map_err(|e| e.to_string())?;
    storage()?
        .set_item("mtl_jobines.save.v1", &json)
        .map_err(|_| "could not write local storage".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_save_round_trip_keeps_facts_and_completion() {
        let bytes = br#"{"version":1,"money_cents":2375,"completed":{"taxi":2,"bus":1,"snowplow":0},"facts":{"visited_stop":true}}"#;
        let progress = decode(bytes).unwrap();
        assert_eq!(progress.money_cents, 2375);
        assert_eq!(progress.completed.taxi, 2);
        assert_eq!(progress.facts["visited_stop"], true);
        assert!(decode(br#"{"version":2,"money_cents":0,"completed":{}}"#).is_err());
    }
}
