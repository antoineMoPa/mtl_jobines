//! Commercial art split into fixed facades and independently breakable assemblies.
use super::commercial::{seed, Family};
use opencity::runtime::{BuildingFrontage, City, Mesh, Region, SurfaceKind, Vertex};
use opencity::BuildingUse;
pub const SIGN: f32 = 40.;
pub const SHOP: f32 = 41.;
pub const BRANDS: [(&str, f32, bool); 10] = [
    ("jean-couteau", 410. / 61., true),
    ("st-hurtubise", 405. / 69., false),
    ("couche-tot", 372. / 60., true),
    ("mini", 235. / 102., false),
    ("ici", 194. / 89., true),
    ("piassorama", 373. / 68., false),
    ("tom-houblon", 401. / 60., true),
    ("belle-patente", 426. / 68., false),
    ("simonac", 339. / 60., true),
    ("cineplouf", 364. / 68., false),
];
#[derive(Clone, Copy)]
enum RetailFacade {
    Flagship,
    Department,
    Cinema,
    Heritage,
}
impl RetailFacade {
    fn from_seed(id: u64) -> Self {
        match (id >> 8) % 4 {
            0 => Self::Flagship,
            1 => Self::Department,
            2 => Self::Cinema,
            _ => Self::Heritage,
        }
    }
    fn brands(self) -> &'static [usize] {
        match self {
            Self::Flagship => &[0, 8],
            Self::Department => &[8, 3, 4],
            Self::Cinema => &[9],
            Self::Heritage => &[8, 0, 6],
        }
    }
}
fn quad(
    mesh: &mut Mesh,
    f: BuildingFrontage,
    p: [[f32; 3]; 4],
    color: [f32; 3],
    kind: f32,
    record: f32,
    uv: [[f32; 2]; 4],
) {
    for i in [0, 1, 2, 0, 2, 3] {
        mesh.vertices.push(Vertex {
            position: f.point(p[i][0], p[i][1], p[i][2]),
            normal: [f.outward[0], f.outward[1], 0.],
            color,
            surface: [
                kind,
                if kind == SIGN && f.along[0] * f.outward[1] - f.along[1] * f.outward[0] > 0. {
                    1. - uv[i][0]
                } else {
                    uv[i][0]
                },
                uv[i][1],
                record,
            ],
        });
    }
}
fn panel(
    mesh: &mut Mesh,
    f: BuildingFrontage,
    x: f32,
    z: f32,
    w: f32,
    h: f32,
    y: f32,
    c: [f32; 3],
    kind: f32,
    record: f32,
    u: [f32; 2],
) {
    quad(
        mesh,
        f,
        [[x, y, z], [x + w, y, z], [x + w, y, z + h], [x, y, z + h]],
        c,
        kind,
        record,
        [[u[0], 1.], [u[1], 1.], [u[1], 0.], [u[0], 0.]],
    );
}
pub(super) fn box_local(
    mesh: &mut Mesh,
    f: BuildingFrontage,
    min: [f32; 3],
    max: [f32; 3],
    color: [f32; 3],
) {
    let mut local = Mesh::default();
    local.cuboid(min, max, color, SurfaceKind::Prop);
    for mut v in local.vertices {
        let n = v.normal;
        v.position = f.point(v.position[0], v.position[1], v.position[2]);
        v.normal = [
            f.along[0] * n[0] + f.outward[0] * n[1],
            f.along[1] * n[0] + f.outward[1] * n[1],
            n[2],
        ];
        mesh.vertices.push(v);
    }
}
#[cfg(test)]
pub(super) fn build(city: &City) -> Mesh {
    parts(city).0
}
fn roof_skin(
    w: f32,
    d: f32,
    h: f32,
    family: Family,
    style: opencity::BuildingStyle,
    base: f32,
) -> Mesh {
    let mut mesh = Mesh::default();
    let slate = [0.12, 0.15, 0.15];
    if matches!(family, Family::Conversion) {
        let inset = (w * 0.15).min(0.012);
        let shoulder = (d * 0.25).min(0.018);
        let eave = [[0., 0., h], [w, 0., h], [w, -d, h], [0., -d, h]];
        let top = [
            [inset, -shoulder, h + 0.018],
            [w - inset, -shoulder, h + 0.018],
            [w - inset, -d + shoulder, h + 0.018],
            [inset, -d + shoulder, h + 0.018],
        ];
        for side in 0..4 {
            let next = (side + 1) % 4;
            mesh.quad(
                [eave[side], eave[next], top[next], top[side]],
                slate,
                SurfaceKind::Roof,
            );
        }
        mesh.quad(top, slate, SurfaceKind::Roof);
    } else {
        let ridge = h + 0.027;
        mesh.quad(
            [
                [0., 0., h],
                [w, 0., h],
                [w, -d / 2., ridge],
                [0., -d / 2., ridge],
            ],
            slate,
            SurfaceKind::Roof,
        );
        mesh.quad(
            [
                [0., -d / 2., ridge],
                [w, -d / 2., ridge],
                [w, -d, h],
                [0., -d, h],
            ],
            slate,
            SurfaceKind::Roof,
        );
        let wall = style
            .material
            .wall_rgb()
            .map(|c| (f32::from(c) / 255.).powf(2.2));
        let data = [base, style.floor_height, style.material.shader_id() as f32];
        mesh.triangle(
            [[0., 0., h], [0., -d, h], [0., -d / 2., ridge]],
            wall,
            SurfaceKind::Building,
            data,
        );
        mesh.triangle(
            [[w, 0., h], [w, -d / 2., ridge], [w, -d, h]],
            wall,
            SurfaceKind::Building,
            data,
        );
    }
    mesh
}
#[cfg(test)]
pub fn prop_groups(city: &City) -> Vec<Vec<Mesh>> {
    parts(city).1
}
pub(super) fn parts(city: &City) -> (Mesh, Vec<Vec<Mesh>>) {
    parts_observed(city, |_, _, _| {})
}
#[cfg(feature = "memory-profile")]
pub(super) fn memory_by_building(city: &City) -> Vec<opencity::runtime::BuildingDetailMemory> {
    let mut rows = Vec::new();
    parts_observed(city, |feature, scenery_vertices, groups| {
        rows.push(opencity::runtime::BuildingDetailMemory {
            feature,
            scenery_vertices,
            prop_vertices: groups.iter().flatten().map(|m| m.vertices.len()).sum(),
            prop_capacity_bytes: groups
                .iter()
                .flatten()
                .map(|m| m.vertices.capacity() * std::mem::size_of::<Vertex>())
                .sum(),
            prop_meshes: groups.iter().map(Vec::len).sum(),
        });
    });
    rows
}
fn parts_observed(
    city: &City,
    mut observe: impl FnMut(usize, usize, &[Vec<Mesh>]),
) -> (Mesh, Vec<Vec<Mesh>>) {
    let mut mesh = Mesh::default();
    let mut props = Vec::new();
    for (feature_id, feature) in city.query(Region::WholeMap).buildings {
        let scenery_start = mesh.vertices.len();
        let props_start = props.len();
        if !matches!(
            feature.building_use,
            BuildingUse::Commercial | BuildingUse::MixedUse
        ) || feature.model.is_some()
            // The section kit supplies its own street floor and facade. Legacy
            // panels follow the lot edge and would float in front of setbacks.
            || opencity::runtime::architecture::BuildingRecipe::from_feature(feature).is_some()
        {
            continue;
        }
        let Some(f) = city.building_frontage(feature, 0.3) else {
            continue;
        };
        let name = feature.name.as_deref().unwrap_or("authored_shop");
        if !super::detailed_frontage(feature) {
            // Interior developed areas retain visible shops without repeating
            // every street-front reveal, fitting and breakable display assembly.
            let door = (f.width * 0.16).min(0.018);
            panel(
                &mut mesh,
                f,
                0.002,
                0.003,
                f.width - door - 0.005,
                0.029,
                0.0008,
                [0.18, 0.21, 0.18],
                SHOP,
                (seed(name) % 3) as f32,
                [0., 1.],
            );
            panel(
                &mut mesh,
                f,
                f.width - door,
                0.001,
                door - 0.002,
                0.03,
                0.001,
                [0.10, 0.15, 0.14],
                8.,
                0.,
                [0., 1.],
            );
            observe(
                feature_id,
                mesh.vertices.len() - scenery_start,
                &props[props_start..],
            );
            continue;
        }
        let id = name
            .strip_prefix("generated_shop_")
            .and_then(|n| u64::from_str_radix(n, 16).ok())
            .unwrap_or_else(|| seed(name));
        let family = Family::from_seed(id);
        let retail_facade = RetailFacade::from_seed(id);
        let w = f.width;
        let h = feature.height.unwrap_or(0.10);
        let trim = [0.23, 0.20, 0.15];
        let metal = [0.055, 0.075, 0.065];
        if let Some(board) = super::commerce_props::sandwich_board(city, f, name) {
            props.push(vec![board]);
        }
        let stone = [0.43, 0.39, 0.30];
        // Pane is 8 cm in front of the solid collision shell, recessed behind 22 cm reveals.
        let door = (w * 0.14).clamp(0.009, 0.016);
        let shopw = w - door - 0.006;
        let bays = (shopw / 0.035).ceil().max(1.) as usize;
        let recipe = match family {
            Family::Commercial | Family::Warehouse => 2.,
            _ => ((id >> 24) % 3) as f32,
        };
        for bay in 0..bays {
            let x = 0.002 + shopw * bay as f32 / bays as f32;
            let bw = shopw / bays as f32;
            panel(
                &mut mesh,
                f,
                x,
                0.003,
                bw - 0.0015,
                0.029,
                0.0008,
                [0.18, 0.21, 0.18],
                SHOP,
                recipe,
                [bay as f32 / bays as f32, (bay as f32 + 1.) / bays as f32],
            );
            box_local(
                &mut mesh,
                f,
                [x, 0.0005, 0.001],
                [x + 0.0015, 0.0022, 0.033],
                metal,
            );
        }
        // Independent upper-floor entrance and shop door hardware.
        panel(
            &mut mesh,
            f,
            w - door,
            0.001,
            door - 0.002,
            0.030,
            0.001,
            trim,
            6.,
            0.,
            [0., 1.],
        );
        panel(
            &mut mesh,
            f,
            w - door + 0.002,
            0.010,
            door - 0.006,
            0.017,
            0.0014,
            [0.08, 0.12, 0.13],
            8.,
            0.,
            [0., 1.],
        );
        box_local(
            &mut mesh,
            f,
            [w - door + 0.002, 0.0015, 0.011],
            [w - door + 0.0025, 0.0028, 0.017],
            stone,
        );
        box_local(&mut mesh, f, [0., 0., 0.], [w, 0.0025, 0.003], stone);
        box_local(&mut mesh, f, [0., 0., 0.032], [w, 0.0025, 0.041], trim);
        box_local(&mut mesh, f, [w - 0.002, 0., 0.], [w, 0.0022, 0.041], stone);
        // Sparse approval artwork uses an independent hash stream and exact aspect.
        let brand_seed = seed(&format!("{name}:branding"));
        if brand_seed % 10 == 0 {
            let brand = if matches!(family, Family::Commercial) {
                let choices = retail_facade.brands();
                choices[((brand_seed >> 16) as usize) % choices.len()]
            } else {
                ((brand_seed >> 16) % 8) as usize
            };
            let (_, aspect, _) = BRANDS[brand];
            let sw = (w * 0.83).min(0.008 * aspect);
            let sh = sw / aspect;
            panel(
                &mut mesh,
                f,
                (w - sw) / 2.,
                0.036 - sh / 2.,
                sw,
                sh,
                0.0028,
                [1.; 3],
                SIGN,
                brand as f32,
                [0., 1.],
            );
        }
        let floors = ((h - 0.042) / family.floor_height()).round().max(0.) as usize;
        for floor in 0..floors {
            let z = 0.046 + floor as f32 * family.floor_height();
            let bh = family.floor_height() * 0.65;
            let broad = matches!(family, Family::Commercial)
                && !matches!(retail_facade, RetailFacade::Heritage);
            let n = (w / if broad { 0.055 } else { 0.026 }).floor().max(2.) as usize;
            for bay in 0..n {
                let x = (bay as f32 + 0.2) * w / n as f32;
                let bw = w / n as f32 * if broad { 0.82 } else { 0.60 };
                // Four reveal strips leave the aperture open; no solid plate covers the pane.
                for (min, max) in [
                    ([x - 0.001, 0., z - 0.001], [x, 0.003, z + bh + 0.001]),
                    (
                        [x + bw, 0., z - 0.001],
                        [x + bw + 0.001, 0.003, z + bh + 0.001],
                    ),
                    ([x, 0., z - 0.001], [x + bw, 0.003, z]),
                    ([x, 0., z + bh], [x + bw, 0.003, z + bh + 0.001]),
                ] {
                    box_local(&mut mesh, f, min, max, stone);
                }
                let retail = matches!(family, Family::Commercial);
                panel(
                    &mut mesh,
                    f,
                    x,
                    z,
                    bw,
                    bh,
                    0.0021,
                    [0.12, 0.17, 0.18],
                    if retail { SHOP } else { 8. },
                    if retail {
                        2.
                    } else {
                        16. * (1. + ((id >> 32) % 256) as f32) + bw / bh
                    },
                    if retail {
                        [x / w, (x + bw) / w]
                    } else {
                        [0., 1.]
                    },
                );
            }
        }
        if matches!(family, Family::Commercial) && !matches!(retail_facade, RetailFacade::Cinema) {
            let count = if matches!(retail_facade, RetailFacade::Flagship) {
                1
            } else {
                2
            };
            for i in 0..count {
                panel(
                    &mut mesh,
                    f,
                    w * (0.12 + i as f32 * 0.63),
                    0.050,
                    w * 0.13,
                    (h - 0.062).max(0.035),
                    0.004,
                    [0.13, 0.20, 0.20],
                    6.,
                    0.,
                    [0., 1.],
                );
            }
        }
        if matches!(family, Family::Commercial) && matches!(retail_facade, RetailFacade::Cinema) {
            panel(
                &mut mesh,
                f,
                w * 0.10,
                0.048,
                w * 0.80,
                (h - 0.055).max(0.02),
                0.004,
                [0.12, 0.17, 0.16],
                6.,
                0.,
                [0., 1.],
            );
            for i in 0..3 {
                panel(
                    &mut mesh,
                    f,
                    w * (0.16 + i as f32 * 0.25),
                    0.063,
                    w * 0.18,
                    (h - 0.08).max(0.02),
                    0.005,
                    [0.27, 0.30, 0.24],
                    6.,
                    0.,
                    [0., 1.],
                );
            }
            box_local(
                &mut mesh,
                f,
                [0., 0., 0.033],
                [w, 0.018, 0.041],
                [0.12, 0.19, 0.18],
            );
            box_local(
                &mut mesh,
                f,
                [0., 0.018, 0.033],
                [w, 0.019, 0.035],
                [0.48, 0.37, 0.20],
            );
        }
        match family {
            Family::Canopy | Family::Strip => {
                let mut roof = Mesh::default();
                box_local(
                    &mut roof,
                    f,
                    [-0.001, 0.001, 0.030],
                    [w + 0.001, 0.022, 0.032],
                    [0.15, 0.23, 0.22],
                );
                let mut assembly = vec![roof];
                for x in [0.003, w - 0.003] {
                    let mut post = Mesh::default();
                    box_local(
                        &mut post,
                        f,
                        [x, 0.020, 0.],
                        [x + 0.001, 0.021, 0.030],
                        metal,
                    );
                    assembly.push(post);
                }
                props.push(assembly);
            }
            Family::Conversion | Family::Village => {
                if matches!(family, Family::Conversion) {
                    let dw = (w * 0.25).min(0.025);
                    let x = (w - dw) / 2.;
                    box_local(
                        &mut mesh,
                        f,
                        [x, -0.014, h + 0.002],
                        [x + dw, 0.001, h + 0.013],
                        stone,
                    );
                    panel(
                        &mut mesh,
                        f,
                        x + 0.003,
                        h + 0.004,
                        dw - 0.006,
                        0.007,
                        0.0011,
                        [0.08, 0.12, 0.14],
                        8.,
                        16. + (dw - 0.006) / 0.007,
                        [0., 1.],
                    );
                    box_local(
                        &mut mesh,
                        f,
                        [x - 0.001, -0.015, h + 0.013],
                        [x + dw + 0.001, 0.002, h + 0.015],
                        trim,
                    );
                }
                let local = roof_skin(
                    w,
                    f.depth,
                    h,
                    family,
                    feature.building_style.unwrap_or_default(),
                    f.base,
                );
                for mut v in local.vertices {
                    let n = v.normal;
                    v.position = f.point(v.position[0], v.position[1], v.position[2]);
                    v.normal = [
                        f.along[0] * n[0] + f.outward[0] * n[1],
                        f.along[1] * n[0] + f.outward[1] * n[1],
                        n[2],
                    ];
                    mesh.vertices.push(v);
                }
            }
            _ => {
                box_local(
                    &mut mesh,
                    f,
                    [-0.002, -0.003, h - 0.003],
                    [w + 0.002, 0.004, h + 0.003],
                    stone,
                );
                box_local(
                    &mut mesh,
                    f,
                    [-0.001, -0.002, h + 0.003],
                    [w + 0.001, 0.002, h + 0.006],
                    trim,
                );
            }
        }
        observe(
            feature_id,
            mesh.vertices.len() - scenery_start,
            &props[props_start..],
        );
    }
    (mesh, props)
}

#[cfg(test)]
mod tests {
    #[test]
    fn procedural_facades_do_not_receive_legacy_shop_windows_or_props() {
        for family in opencity::BuildingFamily::ALL {
            for points in ["0,0,0.3,0,0.3,0.2,0,0.2", "0,0,0,0.2,0.3,0.2,0.3,0"] {
                let source = format!(
                    "shop=building(0.65,{{material:glass,family:{},use:mixed_use,\
                    floor_height:0.0325,front_edge:0}}, {points});",
                    family.keyword()
                );
                let city = City::from_program(&opencity::parse(&source).unwrap(), None).unwrap();
                let (details, props) = parts(&city);
                assert!(
                    details.vertices.is_empty(),
                    "{family:?}: floating lot-edge details"
                );
                assert!(props.is_empty(), "{family:?}: duplicate street-floor props");
                let body = city.mesh(Region::WholeMap, 0.05).unwrap();
                assert!(body
                    .vertices
                    .iter()
                    .any(|v| v.surface[0] == SurfaceKind::Facade as u32 as f32));
            }
        }
        let city = City::from_program(
            &opencity::parse(
                "shop=building(0.12,{family:legacy,use:commercial},0,0,0.3,0,0.3,0.2,0,0.2);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let (details, _) = parts(&city);
        assert!(details.vertices.iter().any(|v| v.surface[0] == SHOP));
    }
    #[test]
    fn developed_area_shops_keep_storefronts_with_a_small_mesh() {
        let p = opencity::parse(
            "road local: carlane; end; local(-1,0,1,0);
            generated_area_0=building(0.12,{use:mixed_use},-0.1,0.1,0.1,0.1,0.1,0.3,-0.1,0.3);",
        )
        .unwrap();
        let city = opencity::runtime::City::from_program(&p, None).unwrap();
        let mesh = super::build(&city);
        assert!(mesh.vertices.iter().any(|v| v.surface[0] == super::SHOP));
        assert!(mesh.vertices.len() <= 24);
        assert!(city.building_collision([0., 0.2], 0.01, 0., 0.1));
    }
    use super::*;
    #[test]
    fn roof_skins_have_no_open_edges_above_the_wall_top() {
        use std::collections::BTreeMap;
        for family in [Family::Conversion, Family::Village] {
            for (w, d) in [(0.045, 0.055), (0.08, 0.145), (0.20, 0.21)] {
                let height = 0.11;
                let roof = roof_skin(
                    w,
                    d,
                    height,
                    family,
                    opencity::BuildingStyle::default(),
                    0.1,
                );
                let mut edges = BTreeMap::new();
                for t in roof.vertices.chunks_exact(3) {
                    for i in 0..3 {
                        let mut key = [
                            t[i].position.map(f32::to_bits),
                            t[(i + 1) % 3].position.map(f32::to_bits),
                        ];
                        key.sort();
                        *edges.entry(key).or_insert(0) += 1;
                    }
                }
                let mut eaves = 0;
                for (edge, count) in edges {
                    assert!(count == 1 || count == 2, "nonmanifold roof");
                    if count == 1 {
                        eaves += 1;
                        for p in edge {
                            assert_eq!(
                                f32::from_bits(p[2]),
                                height,
                                "open attic edge on {family:?}"
                            );
                        }
                    }
                }
                assert_eq!(
                    eaves, 4,
                    "only the four wall-top edges remain open underneath"
                );
                assert!(roof.vertices.iter().all(|v| v.position[0] >= 0.
                    && v.position[0] <= w
                    && v.position[1] >= -d
                    && v.position[1] <= 0.
                    && v.position[2] >= height));
            }
        }
    }
    #[test]
    fn pane_uv_derivatives_recover_authored_direction_on_both_windings() {
        for outward in [[0., 1.], [0., -1.], [1., 0.], [-1., 0.]] {
            for along in [[outward[1], -outward[0]], [-outward[1], outward[0]]] {
                let f = BuildingFrontage {
                    origin: [0., 0.],
                    along,
                    outward,
                    width: 0.12,
                    depth: 0.14,
                    base: 0.02,
                };
                let mut mesh = Mesh::default();
                panel(
                    &mut mesh,
                    f,
                    0.02,
                    0.003,
                    0.04,
                    0.029,
                    0.0008,
                    [1.; 3],
                    SHOP,
                    1.,
                    [0.2, 0.6],
                );
                let a = mesh.vertices[0];
                let b = mesh.vertices[1];
                let derivative =
                    glam::Vec3::from_array(b.position) - glam::Vec3::from_array(a.position);
                let tangent = derivative / derivative.length();
                assert!((tangent.dot(glam::Vec3::new(along[0], along[1], 0.)) - 1.).abs() < 1e-5);
                assert!(b.surface[1] > a.surface[1]);
                for v in mesh.vertices {
                    let offset = [v.position[0] - f.origin[0], v.position[1] - f.origin[1]];
                    let outside = offset[0] * outward[0] + offset[1] * outward[1];
                    assert!(
                        (outside - 0.0008).abs() < 1e-6,
                        "pane must remain outside solid shell"
                    );
                }
            }
        }
    }
    #[test]
    fn signs_read_left_to_right_when_viewed_from_either_side() {
        for outward in [[0., 1.], [0., -1.], [1., 0.], [-1., 0.]] {
            for along in [[outward[1], -outward[0]], [-outward[1], outward[0]]] {
                let f = BuildingFrontage {
                    origin: [0., 0.],
                    along,
                    outward,
                    width: 0.12,
                    depth: 0.14,
                    base: 0.02,
                };
                let mut mesh = Mesh::default();
                panel(
                    &mut mesh,
                    f,
                    0.,
                    0.03,
                    0.05,
                    0.01,
                    0.003,
                    [1.; 3],
                    SIGN,
                    0.,
                    [0., 1.],
                );
                let a = mesh.vertices[0];
                let b = mesh.vertices[1];
                let du = b.surface[1] - a.surface[1];
                let right = glam::Vec3::new(-outward[1], outward[0], 0.);
                let delta = glam::Vec3::from_array(b.position) - glam::Vec3::from_array(a.position);
                assert!(delta.dot(right) / du > 0.);
            }
        }
    }
}
