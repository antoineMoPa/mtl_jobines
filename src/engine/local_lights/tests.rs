use super::*;

#[test]
fn spatial_masks_cover_every_light_range_across_all_four_words() {
    assert!(std::mem::size_of::<LightGrid>() <= 16384);
    let sources: Vec<_> = (0..128)
        .map(|i| {
            LocalLight::streetlamp(Vec3::new(
                (i % 16) as f32 * 0.12 - 0.9,
                (i / 16) as f32 * 0.12 - 0.4,
                0.055,
            ))
        })
        .collect();
    let grid = grid(&sources, Vec3::ZERO);
    for (index, light) in grid.lights.iter().enumerate() {
        let radius = light.position_range[3];
        for y in 0..GRID {
            for x in 0..GRID {
                let p = glam::Vec2::new(grid.origin[0], grid.origin[1])
                    + glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5) * CELL;
                if p.distance(light.position().truncate()) <= radius {
                    assert_ne!(
                        grid.cells[y * GRID + x][index / 32] & (1 << (index % 32)),
                        0
                    );
                }
            }
        }
    }
    assert!(grid.cells.iter().any(|cell| cell[3] != 0));
}

#[test]
fn grid_prioritizes_nearby_sources_and_clears_removed_lights() {
    let mut sources = vec![LocalLight::streetlamp(Vec3::new(1., 0., 0.05)); 200];
    sources.push(LocalLight::headlight(Vec3::ZERO, glam::Quat::IDENTITY, 1.));
    sources.push(LocalLight::streetlamp(Vec3::splat(100.)));
    let selected = grid(&sources, Vec3::ZERO);
    assert_eq!(selected.lights[0].position(), Vec3::ZERO);
    assert!(selected.lights.iter().all(|l| l.position().x < 2.));
    let empty = grid(&[], Vec3::ZERO);
    assert!(empty.cells.iter().all(|cell| *cell == [0; 4]));
}

#[cfg(not(target_arch = "wasm32"))]
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
mod rear_gpu;
