use super::*;

impl Document {
    fn ground_point(&self, city: &City, view: View, pos: egui::Pos2) -> Option<Vec2> {
        let mut height = self.center.z;
        let mut point = view.ground(pos, height)?;
        for _ in 0..4 {
            height = self.ground_height(city, point);
            point = view.ground(pos, height)?;
        }
        Some(point)
    }
    pub(super) fn pick(&self, city: &City, view: View, pos: egui::Pos2) -> Option<usize> {
        let (index, distance) = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| geometry::mesh_hit(view, pos, &item.mesh).map(|t| (i, t)))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        for (_, feature) in city
            .query(Region::Radius {
                center: [self.center.x, self.center.y],
                radius: self.span * 2.,
            })
            .buildings
        {
            if self
                .items
                .iter()
                .any(|item| item.feature.name == feature.name)
            {
                continue;
            }
            if geometry::mesh_hit(view, pos, &geometry::prism(feature))
                .is_some_and(|t| t < distance - 1e-6)
            {
                return None;
            }
        }
        Some(index)
    }
    pub(super) fn interaction(
        &mut self,
        ctx: &egui::Context,
        city: &City,
        view: View,
        candidates: &[LandscapeFeature],
    ) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.draft = None;
            if self.gesture.take().is_none() && self.selected.take().is_none() {
                self.open = false;
            }
            return;
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if let Some(index) = self.selected {
            let delta = ctx.input(|i| {
                Vec2::new(
                    f32::from(i.key_pressed(egui::Key::ArrowRight))
                        - f32::from(i.key_pressed(egui::Key::ArrowLeft)),
                    f32::from(i.key_pressed(egui::Key::ArrowUp))
                        - f32::from(i.key_pressed(egui::Key::ArrowDown)),
                )
            }) * 0.0025;
            if delta != Vec2::ZERO {
                let feature = geometry::translated(&self.items[index].feature, delta);
                self.error = self.edit(city, index, feature).err().unwrap_or_default();
            }
        }
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else {
            if ctx.input(|i| !i.pointer.primary_down() && !i.pointer.secondary_down()) {
                self.gesture = None;
                self.draft = None;
            }
            return;
        };
        let in_view = view.viewport.contains(pos);
        let primary_pressed = ctx.input(|i| i.pointer.primary_pressed());
        let primary_released = ctx.input(|i| i.pointer.primary_released());
        let secondary_pressed = ctx.input(|i| i.pointer.secondary_pressed());
        if in_view && self.mode == CameraMode::Build {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. && self.gesture.is_none() {
                self.span = (self.span * (-scroll * 0.002).exp()).clamp(0.2, 8.);
                ctx.request_repaint();
            }
            if secondary_pressed {
                self.gesture = Some(Gesture::Pan {
                    press: pos,
                    center: self.center,
                });
            }
        }
        if primary_pressed && in_view {
            self.error.clear();
            let corner = self.selected.and_then(|index| {
                let f = &self.items[index].feature;
                (self.mode == CameraMode::Build)
                    .then(|| {
                        f.points.iter().enumerate().find_map(|(j, p)| {
                            let screen =
                                view.project(Vec3::new(p[0], p[1], f.elevation + 0.002))?;
                            (screen.distance(pos) < 10.).then_some((index, j))
                        })
                    })
                    .flatten()
            });
            if let Some((index, corner)) = corner {
                let before = self.items[index].feature.clone();
                self.gesture = Some(Gesture::Resize {
                    index,
                    opposite: Vec2::from_array(before.points[(corner + 2) % 4]),
                    before,
                    press: pos,
                    active: false,
                });
            } else if let Some(index) = self.pick(city, view, pos) {
                self.selected = Some(index);
                self.gesture = Some(Gesture::Move {
                    index,
                    before: self.items[index].feature.clone(),
                    press: pos,
                    active: false,
                });
            } else {
                let candidate = candidates
                    .iter()
                    .filter_map(|f| {
                        view.project(center(f).extend(f.elevation + 0.002))
                            .map(|p| (p.distance(pos), f))
                    })
                    .filter(|(distance, _)| *distance < 16.)
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, f)| f.clone());
                if let Some(feature) = candidate {
                    let feature = self.prototype(city, feature.points);
                    self.error = self.create(city, feature).err().unwrap_or_default();
                } else if self.mode == CameraMode::Build {
                    if let Some(start) = self.ground_point(city, view, pos) {
                        self.selected = None;
                        self.gesture = Some(Gesture::Draw {
                            start,
                            press: pos,
                            current: start,
                        });
                    }
                } else {
                    self.selected = None;
                }
            }
        }
        let ceiling = self.surface_ceiling();
        let draw_point = self.ground_point(city, view, pos);
        let mut commit = None;
        if let Some(gesture) = &mut self.gesture {
            match gesture {
                Gesture::Pan { press, center } => {
                    if let (Some(from), Some(to)) =
                        (view.ground(*press, center.z), view.ground(pos, center.z))
                    {
                        self.center = *center + (from - to).extend(0.);
                        self.center.z = city
                            .surface_at(self.center.truncate().to_array(), f32::INFINITY)
                            .map_or(self.center.z, |surface| surface.height);
                        *press = pos;
                        *center = self.center;
                        ctx.request_repaint();
                    }
                    if ctx.input(|i| i.pointer.secondary_released()) {
                        self.gesture = None;
                    }
                }
                Gesture::Draw {
                    start,
                    press,
                    current,
                } => {
                    if pos.distance(*press) >= 4. {
                        if let Some(point) = draw_point {
                            *current = point;
                        }
                        let points = geometry::rectangle(*start, *current);
                        let c = points.iter().map(|p| Vec2::from_array(*p)).sum::<Vec2>() / 4.;
                        let z = city
                            .surface_at(c.to_array(), ceiling)
                            .map_or(self.player.z, |s| s.height);
                        self.draft = Some(blank(points, z + 0.001));
                    }
                    if primary_released {
                        commit = Some(None);
                    }
                }
                Gesture::Move {
                    index,
                    before,
                    press,
                    active,
                } => {
                    *active |= pos.distance(*press) >= 4.;
                    if *active {
                        let delta = if self.mode == CameraMode::Build {
                            view.ground(pos, before.elevation)
                                .zip(view.ground(*press, before.elevation))
                                .map(|(to, from)| to - from)
                                .unwrap_or_default()
                        } else {
                            let pixels = pos - *press;
                            let f = Vec2::new(self.heading.cos(), self.heading.sin());
                            Vec2::new(f.y, -f.x) * (pixels.x * 0.0004) - f * (pixels.y * 0.0004)
                        };
                        let delta = (delta / 0.0025).round() * 0.0025;
                        let mut feature = geometry::translated(before, delta);
                        feature.elevation = city
                            .surface_at(center(&feature).to_array(), ceiling)
                            .map_or(before.elevation, |surface| surface.height + 0.001);
                        self.draft = Some(feature);
                        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    if primary_released {
                        commit = Some(Some(*index));
                    }
                }
                Gesture::Resize {
                    index,
                    before,
                    opposite,
                    press,
                    active,
                } => {
                    *active |= pos.distance(*press) >= 4.;
                    if let Some(point) = view.ground(pos, before.elevation).filter(|_| *active) {
                        let edge =
                            Vec2::from_array(before.points[1]) - Vec2::from_array(before.points[0]);
                        let angle = edge.y.atan2(edge.x);
                        let (s, c) = angle.sin_cos();
                        let delta = point - *opposite;
                        let local =
                            Vec2::new(c * delta.x + s * delta.y, -s * delta.x + c * delta.y);
                        let mut feature = before.clone();
                        feature.points = geometry::rectangle(Vec2::ZERO, local)
                            .into_iter()
                            .map(|p| {
                                (*opposite + Vec2::new(c * p[0] - s * p[1], s * p[0] + c * p[1]))
                                    .to_array()
                            })
                            .collect();
                        self.draft = Some(feature);
                    }
                    if primary_released {
                        commit = Some(Some(*index));
                    }
                }
            }
        } else if in_view && self.pick(city, view, pos).is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::Grab);
        }
        if let Some(index) = commit {
            self.gesture = None;
            if let Some(feature) = self.draft.take() {
                let result = if let Some(index) = index {
                    self.edit(city, index, feature)
                } else {
                    let feature = self.prototype(city, feature.points);
                    self.create(city, feature)
                };
                self.error = result.err().unwrap_or_default();
            }
            ctx.request_repaint();
        }
    }
}
pub(super) fn blank(points: Vec<[f32; 2]>, elevation: f32) -> LandscapeFeature {
    LandscapeFeature {
        kind: LandscapeKind::Building,
        name: None,
        points,
        elevation,
        height: Some(0.1),
        building_style: Some(BuildingStyle::default()),
        building_use: opencity::BuildingUse::Unspecified,
        front_edge: None,
        terrain_height: None,
        model: None,
        vegetation_type: None,
    }
}
