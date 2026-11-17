use super::*;
impl Document {
    fn pick(&self, view: View, pos: egui::Pos2) -> Option<Member> {
        let (origin, direction) = view.ray(pos);
        self.samples
            .iter()
            .filter_map(|s| {
                let transform = s.placement.transform();
                s.placement
                    .parts()
                    .iter()
                    .flat_map(|part| part.mesh.vertices.chunks_exact(3))
                    .filter_map(|triangle| {
                        geometry::triangle_hit(
                            origin,
                            direction,
                            std::array::from_fn(|i| {
                                transform.transform_point3(Vec3::from_array(triangle[i].position))
                            }),
                        )
                    })
                    .min_by(f32::total_cmp)
                    .map(|t| (s.member, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(m, _)| m)
            .or_else(|| {
                self.samples
                    .iter()
                    .filter_map(|s| {
                        view.project(s.placement.position + Vec3::Z * 0.002)
                            .map(|p| (s.member, p.distance(pos)))
                    })
                    .filter(|(_, d)| *d < 10.)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(m, _)| m)
            })
    }
    fn handle(&self, view: View, pos: egui::Pos2) -> Option<usize> {
        let row = self.row()?;
        [row.a, row.b, row.control]
            .iter()
            .enumerate()
            .find_map(|(i, p)| {
                view.project(p.extend(row.elevation + 0.002))
                    .filter(|p| p.distance(pos) < 10.)
                    .map(|_| i)
            })
    }
    pub(super) fn move_members(&mut self, city: &City, members: &[Member], delta: Vec2) {
        for m in members {
            match &mut self.entries[m.entry].shape {
                Shape::Single(p) => {
                    p.position += delta.extend(0.);
                    if let Some(s) =
                        city.surface_at(p.position.truncate().to_array(), p.position.z + 0.08)
                    {
                        p.position.z = s.height;
                    }
                }
                Shape::Row { row, .. } => {
                    let offset = row.offsets.entry(m.index).or_default();
                    *offset += delta.extend(0.);
                }
            }
        }
    }
    fn commit_draft(&mut self, city: &City) {
        if let Some(entry) = self.draft.take() {
            if let Shape::Row { row, .. } = &entry.shape {
                if row.a.distance(row.b) < 0.003 {
                    self.error = "Row is too short".into();
                    return;
                }
            }
            let before = self.entries.clone();
            self.entries.push(entry);
            self.selected_row = Some(self.entries.len() - 1);
            self.selected.clear();
            self.tool = Tool::Select;
            self.accept(city, before);
        }
    }
    pub(super) fn interaction(&mut self, ctx: &egui::Context, city: &City, view: View) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            let busy = self.gesture.is_some() || self.draft.is_some();
            self.cancel_interaction();
            self.rebuild(city);
            if !busy {
                if !self.selected.is_empty() || self.selected_row.is_some() {
                    self.selected.clear();
                    self.selected_row = None;
                    self.tool = Tool::Select;
                } else if self.tool != Tool::Select {
                    self.tool = Tool::Select;
                } else {
                    self.open = false;
                }
            }
            return;
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                )
            }) {
                self.history(city, true);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                self.history(city, false);
            }
            if ctx
                .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
            {
                self.delete(city);
            }
            let nudge = ctx.input(|i| {
                Vec2::new(
                    f32::from(i.key_pressed(egui::Key::ArrowRight))
                        - f32::from(i.key_pressed(egui::Key::ArrowLeft)),
                    f32::from(i.key_pressed(egui::Key::ArrowUp))
                        - f32::from(i.key_pressed(egui::Key::ArrowDown)),
                )
            }) * 0.001;
            if nudge != Vec2::ZERO {
                let before = self.entries.clone();
                if let Some(i) = self.selected_row {
                    self.entries[i].translate(nudge);
                } else {
                    self.move_members(city, &self.selected.clone(), nudge);
                }
                self.accept(city, before);
            }
        }
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        let pointer_changed = self.last_pointer != Some(pos);
        self.last_pointer = Some(pos);
        let in_view = view.viewport.contains(pos)
            && ctx
                .layer_id_at(pos)
                .is_none_or(|l| l.order == egui::Order::Background);
        let world = self.point(city, view, pos);
        if in_view && self.mode == Mode::Iso && self.gesture.is_none() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. {
                self.span = (self.span * (-scroll * 0.002).exp()).clamp(0.08, 8.);
                self.rebuild(city);
                ctx.request_repaint();
            }
        }
        let pressed = ctx.input(|i| i.pointer.primary_pressed());
        let released = ctx.input(|i| i.pointer.primary_released());
        if in_view
            && self.mode == Mode::Iso
            && ctx.input(|i| {
                i.pointer.secondary_pressed()
                    || i.pointer.button_pressed(egui::PointerButton::Middle)
            })
        {
            self.gesture = Some(Gesture::Pan {
                press: pos,
                center: self.center,
            });
        }
        if pressed && in_view {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id)
                }
            });
            self.error.clear();
            if let (Some(handle), Some(entry)) = (self.handle(view, pos), self.selected_row) {
                self.gesture = Some(Gesture::Handle {
                    before: self.entries.clone(),
                    entry,
                    handle,
                    press: pos,
                });
            } else if let Some(member) = self.pick(view, pos) {
                if ctx.input(|i| {
                    i.pointer
                        .button_double_clicked(egui::PointerButton::Primary)
                }) && matches!(self.entries[member.entry].shape, Shape::Row { .. })
                {
                    self.selected_row = Some(member.entry);
                    self.selected.clear();
                    self.tool = Tool::Select;
                    return;
                }
                self.selected_row = None;
                if ctx.input(|i| i.modifiers.shift) {
                    if self.selected.contains(&member) {
                        self.selected.retain(|m| *m != member);
                    } else {
                        self.selected.push(member);
                    }
                } else if !self.selected.contains(&member) {
                    self.selected = vec![member];
                }
                if let Some(anchor) = world {
                    self.gesture = Some(Gesture::Move {
                        before: self.entries.clone(),
                        anchor,
                        press: pos,
                        members: self.selected.clone(),
                    });
                }
            } else if let Some(p) = world {
                match self.tool {
                    Tool::Single => match self.new_single(city, p) {
                        Ok(entry) => {
                            let before = self.entries.clone();
                            self.entries.push(entry);
                            self.selected = vec![Member {
                                entry: self.entries.len() - 1,
                                index: 0,
                            }];
                            self.selected_row = None;
                            self.accept(city, before);
                        }
                        Err(e) => self.error = e,
                    },
                    Tool::Row => {
                        if let Some(Entry {
                            shape: Shape::Row { row, .. },
                            ..
                        }) = &mut self.draft
                        {
                            row.b = p;
                            row.control = (row.a + p) * 0.5;
                            self.commit_draft(city);
                        } else {
                            self.draft = self.new_row(city, p).map_err(|e| self.error = e).ok();
                            self.gesture = Some(Gesture::NewRow { press: pos });
                            self.revision += 1;
                        }
                    }
                    Tool::Select => {
                        let path = self.row().is_some_and(|row| {
                            (0..64).any(|i| {
                                view.project(
                                    row.point(i as f32 / 63.).extend(row.elevation + 0.002),
                                )
                                .is_some_and(|p| p.distance(pos) < 6.)
                            })
                        });
                        if path {
                            self.gesture = Some(Gesture::MoveRow {
                                before: self.entries.clone(),
                                entry: self.selected_row.unwrap(),
                                anchor: p,
                                press: pos,
                            });
                        } else {
                            let initial = if ctx.input(|i| i.modifiers.shift) {
                                self.selected.clone()
                            } else {
                                Vec::new()
                            };
                            self.selected = initial.clone();
                            self.selected_row = None;
                            self.gesture = Some(Gesture::Box {
                                start: pos,
                                current: pos,
                                initial,
                            });
                        }
                    }
                }
            } else {
                self.error = "No supported surface here".into();
            }
        }
        if self.gesture.is_none() && in_view {
            if let (
                Some(p),
                Some(Entry {
                    shape: Shape::Row { row, .. },
                    ..
                }),
            ) = (world, &mut self.draft)
            {
                if row.b != p {
                    row.b = p;
                    row.control = (row.a + p) * 0.5;
                    self.revision += 1;
                    ctx.request_repaint();
                }
            }
        }
        let Some(mut gesture) = self.gesture.take() else {
            return;
        };
        let mut keep = true;
        match &mut gesture {
            Gesture::Move {
                before,
                anchor,
                press,
                members,
            } => {
                if pointer_changed && pos.distance(*press) >= 4. {
                    if let Some(p) = world {
                        self.entries = before.clone();
                        self.move_members(city, members, p - *anchor);
                        self.rebuild(city);
                    }
                }
                if released {
                    self.accept(city, before.clone());
                    keep = false;
                }
            }
            Gesture::Handle {
                before,
                entry,
                handle,
                press,
            } => {
                if pointer_changed && pos.distance(*press) >= 4. {
                    if let Some(p) = world {
                        if let Shape::Row { row, .. } = &mut self.entries[*entry].shape {
                            match handle {
                                0 => row.a = p,
                                1 => row.b = p,
                                _ => row.control = p,
                            };
                        }
                        self.rebuild(city);
                    }
                }
                if released {
                    self.accept(city, before.clone());
                    keep = false;
                }
            }
            Gesture::MoveRow {
                before,
                entry,
                anchor,
                press,
            } => {
                if pointer_changed && pos.distance(*press) >= 4. {
                    if let Some(p) = world {
                        self.entries = before.clone();
                        self.entries[*entry].translate(p - *anchor);
                        self.rebuild(city);
                    }
                }
                if released {
                    self.accept(city, before.clone());
                    keep = false;
                }
            }
            Gesture::NewRow { press } => {
                if pointer_changed {
                    if let (
                        Some(p),
                        Some(Entry {
                            shape: Shape::Row { row, .. },
                            ..
                        }),
                    ) = (world, &mut self.draft)
                    {
                        row.b = p;
                        row.control = (row.a + p) * 0.5;
                        self.revision += 1;
                    }
                }
                if released {
                    if pos.distance(*press) >= 4. {
                        self.commit_draft(city);
                    }
                    keep = false;
                }
            }
            Gesture::Box {
                start,
                current,
                initial,
            } => {
                *current = pos;
                if released {
                    let rect = egui::Rect::from_two_pos(*start, pos);
                    self.selected = initial.clone();
                    for sample in &self.samples {
                        if view
                            .project(sample.placement.position)
                            .is_some_and(|p| rect.contains(p))
                            && !self.selected.contains(&sample.member)
                        {
                            self.selected.push(sample.member);
                        }
                    }
                    keep = false;
                }
            }
            Gesture::Pan { press, center } => {
                if pointer_changed {
                    if let (Some(a), Some(b)) =
                        (view.ground(*press, center.z), view.ground(pos, center.z))
                    {
                        self.center = *center + (a - b).extend(0.);
                        self.rebuild(city);
                    }
                }
                if ctx.input(|i| !i.pointer.secondary_down() && !i.pointer.middle_down()) {
                    keep = false;
                }
            }
        }
        if keep {
            self.gesture = Some(gesture);
        }
    }
}
