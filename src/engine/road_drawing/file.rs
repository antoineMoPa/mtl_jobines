//! Source-preserving, conflict-checked saves for a single driving session.
use super::*;
use opencity::{map::SegmentSource, Statement, TokenKind};
use std::path::{Path, PathBuf};
use web_time::{Duration, Instant};

pub(in crate::engine) struct Recording {
    pub drawing: Drawing,
    path: PathBuf,
    base: String,
    prefix: String,
    last_source: String,
    saved_revision: u64,
    saved_at: Instant,
}
impl Recording {
    pub fn new(
        path: &Path,
        city: &City,
        selection: &BTreeSet<LaneEnd>,
        pose: Pose,
    ) -> Result<Self, String> {
        let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let original = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut base = original.clone();
        let mut drawing = Drawing::new(city, selection, pose);
        let mut number = 1;
        let prefix = loop {
            let candidate = format!("driven_{number:04}");
            if !base.contains(&candidate) {
                break candidate;
            }
            number += 1;
        };
        // Give anonymous top-level placements an ID without reformatting authored comments.
        let program = opencity::parse(&base).map_err(|e| e.to_string())?;
        let projection = opencity::map::project(&program);
        for lane in &drawing.lanes {
            let snapshot = &city.roads[lane.id.road].segment;
            if !projection.segments.iter().any(|s| {
                s.name == snapshot.name
                    && (s.name.is_some() || s.source == snapshot.source)
                    && s.road_name == snapshot.road_name
                    && s.start == snapshot.start
                    && s.end == snapshot.end
                    && s.controls == snapshot.controls
                    && s.start_z == snapshot.start_z
                    && s.end_z == snapshot.end_z
            }) {
                return Err(
                    "Map changed before selection; wait for its reload and select again".into(),
                );
            }
        }
        let mut insertions = Vec::new();
        for lane in &mut drawing.lanes {
            if lane.parent.is_some() {
                continue;
            }
            let road = &city.roads[lane.id.road];
            let SegmentSource::TopLevel(statement) = road.segment.source else {
                return Err("Name this road placement in the editor before extending it".into());
            };
            let name = format!("{prefix}_parent_{}", lane.id.road);
            if !insertions
                .iter()
                .any(|(_, id): &(usize, String)| id == &name)
            {
                let Statement::Call(call) = &program.statements[statement] else {
                    return Err("Selected road no longer matches the source file".into());
                };
                let offset = call_offset(&base, call, &program, statement)?;
                insertions.push((offset, name.clone()));
            }
            lane.parent = Some(name);
        }
        insertions.sort_by_key(|(offset, _)| std::cmp::Reverse(*offset));
        for (offset, name) in insertions {
            base.insert_str(offset, &format!("{name} = "));
        }
        Ok(Self {
            drawing,
            path,
            base,
            prefix,
            last_source: original,
            saved_revision: 0,
            saved_at: Instant::now(),
        })
    }
    pub fn owns_source(&self, source: &str) -> bool {
        self.saved_revision > 0 && source == self.last_source
    }
    pub fn saved_source(&self) -> &str {
        &self.last_source
    }
    pub fn save(&mut self, now: Instant, force: bool) -> Result<bool, String> {
        if self.drawing.revision == self.saved_revision
            || (!force && now - self.saved_at < Duration::from_secs(1))
        {
            return Ok(false);
        }
        let current = std::fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
        if current != self.last_source {
            return Err(
                "Map changed outside the road tool; recording stopped to keep those edits".into(),
            );
        }
        let source = format!("{}{}", self.base, self.drawing.source(&self.prefix)?);
        let parsed = opencity::parse(&source).map_err(|e| e.to_string())?;
        let projection = opencity::map::project(&parsed);
        if projection.unresolved != 0 {
            return Err("Road extension has unresolved attachments; recording stopped".into());
        }
        let temporary = self.path.with_extension(format!(
            "road-recording-{}-{}.tmp",
            std::process::id(),
            self.prefix
        ));
        let mut created = false;
        let result = (|| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            created = true;
            file.write_all(source.as_bytes())
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            std::fs::set_permissions(
                &temporary,
                std::fs::metadata(&self.path)
                    .map_err(|e| e.to_string())?
                    .permissions(),
            )
            .map_err(|e| e.to_string())?;
            // Recheck immediately before replacement: external edits always take priority.
            if std::fs::read_to_string(&self.path).map_err(|e| e.to_string())? != current {
                return Err("Map changed during save; recording stopped".into());
            }
            std::fs::rename(&temporary, &self.path).map_err(|e| e.to_string())
        })();
        if created && result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        self.last_source = source;
        self.saved_revision = self.drawing.revision;
        self.saved_at = now;
        Ok(true)
    }
}

fn call_offset(
    source: &str,
    target: &opencity::Call,
    program: &opencity::Program,
    statement: usize,
) -> Result<usize, String> {
    let tokens = opencity::tokenize(source).map_err(|e| e.to_string())?;
    let mut remaining = program.statements[..statement]
        .iter()
        .filter(|s| matches!(s, Statement::Call(c) if c == target))
        .count();
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokenKind::Road | TokenKind::Block)
            || tokens[index].text == "network"
        {
            while index < tokens.len() && tokens[index].kind != TokenKind::End {
                index += 1;
            }
            index += 1;
            continue;
        }
        let start = index;
        while index < tokens.len()
            && !matches!(tokens[index].kind, TokenKind::Semicolon | TokenKind::Eof)
        {
            index += 1;
        }
        if index >= tokens.len() || tokens[index].kind == TokenKind::Eof {
            break;
        }
        let offset = tokens[start].span.offset;
        let end = tokens[index].span.offset + 1;
        if let Ok(parsed) = opencity::parse(&source[offset..end]) {
            if parsed.statements.as_slice() == [Statement::Call(target.clone())] {
                if remaining == 0 {
                    return Ok(offset);
                }
                remaining -= 1;
            }
        }
        index += 1;
    }
    Err("Cannot find the selected anonymous placement in the source".into())
}
