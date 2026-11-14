use super::*;
use std::{
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
};
static SERIAL: AtomicU64 = AtomicU64::new(0);
impl Document {
    pub(super) fn save(&mut self, city: &City) -> Result<(), String> {
        let previous = crate::props::read(&self.program)?;
        for e in &self.entries {
            let modified = !previous.contains(e);
            let placements = e.placements(city)?;
            for (_, p) in placements {
                if !p.position.is_finite() || !p.heading.is_finite() {
                    return Err("Invalid prop placement".into());
                }
                if modified
                    && self.mode == Mode::Street
                    && p.position.truncate().distance(self.player.truncate()) > 0.6
                {
                    return Err("Outside 60 m placement range; use Iso".into());
                }
            }
        }
        let mut after = self.program.clone();
        let mut pending: BTreeMap<_, _> = self
            .entries
            .iter()
            .map(|e| (e.name.clone(), crate::props::write(e)))
            .collect();
        let tokens = opencity::tokenize(&self.source).map_err(|e| e.to_string())?;
        let mut edits = Vec::new();
        let mut statements = Vec::new();
        for (index, s) in self.program.statements.iter().enumerate() {
            let opencity::Statement::Call(call) = s else {
                statements.push(s.clone());
                continue;
            };
            if !matches!(call.name.as_str(), "prop" | "prop_row") {
                statements.push(s.clone());
                continue;
            }
            let replacement = call
                .assigns
                .first()
                .and_then(|n| pending.remove(n))
                .map(opencity::Statement::Call);
            if replacement.as_ref() == Some(s) {
                statements.push(s.clone());
                continue;
            }
            let name = call.assigns.first().ok_or("Prop has no identity")?;
            let mut range = None;
            for (i, token) in tokens.iter().enumerate().filter(|(_, t)| t.text == *name) {
                let Some(end) = tokens[i..]
                    .iter()
                    .find(|t| t.kind == opencity::TokenKind::Semicolon)
                else {
                    continue;
                };
                let r = token.span.offset..end.span.offset + 1;
                if opencity::parse(&self.source[r.clone()])
                    .is_ok_and(|p| p.statements == vec![s.clone()])
                    && opencity::parse(&self.source[..r.start])
                        .is_ok_and(|p| p.statements.len() == index)
                {
                    range = Some(r);
                    break;
                }
            }
            let text = replacement
                .clone()
                .map(|s| {
                    Program {
                        statements: vec![s],
                    }
                    .to_road_source()
                })
                .unwrap_or_default();
            edits.push((range.ok_or("Could not locate authored prop")?, text));
            if let Some(s) = replacement {
                statements.push(s);
            }
        }
        let appended: Vec<_> = self
            .entries
            .iter()
            .filter_map(|e| pending.remove(&e.name))
            .map(opencity::Statement::Call)
            .collect();
        statements.extend(appended.clone());
        after.statements = statements;
        let mut source = self.source.clone();
        edits.sort_by_key(|(r, _)| r.start);
        for (range, text) in edits.into_iter().rev() {
            source.replace_range(range, &text);
        }
        if !appended.is_empty() {
            source.push('\n');
            source.push_str(
                &Program {
                    statements: appended,
                }
                .to_road_source(),
            );
        }
        if opencity::parse(&source).map_err(|e| e.to_string())? != after {
            return Err("Prop source validation failed".into());
        }
        crate::props::read(&after)?;
        let unchanged = || {
            std::fs::read_to_string(&self.path)
                .map_err(|e| e.to_string())
                .and_then(|s| {
                    if s == self.source {
                        Ok(())
                    } else {
                        Err("Map changed on disk; close and reopen Props".into())
                    }
                })
        };
        unchanged()?;
        let temp = self.path.with_extension(format!(
            "road.props-{}-{}.tmp",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut f = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)
                .map_err(|e| e.to_string())?;
            f.write_all(source.as_bytes())
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
            drop(f);
            unchanged()?;
            std::fs::rename(&temp, &self.path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result?;
        self.source = source;
        self.entries = crate::props::read(&after)?;
        self.program = after;
        Ok(())
    }
}
