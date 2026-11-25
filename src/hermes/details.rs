//! Hand off the two products of frontage generation without rebuilding either.
//! A map revision owns this cache; collision props and scenery can consume it in
//! either order (startup creates props first, editor reload prepares scenery first).
use super::{building_style, commerce_mesh, detailed_frontage};
use crate::props::Part;
use opencity::runtime::{City, Mesh, Region};
use std::cell::RefCell;

#[derive(Default)]
pub(super) struct Prepared {
    products: RefCell<Products>,
}
#[derive(Default)]
struct Products {
    scenery: Option<Mesh>,
    props: Option<Vec<Vec<Part>>>,
}
fn build(city: &City) -> Products {
    let (mut scenery, frontage) = city.building_detail_parts_matching(
        Region::WholeMap,
        &building_style::residential(),
        detailed_frontage,
    );
    let (mut commercial, groups) = commerce_mesh::parts(city);
    crate::performance::startup_memory("commercial details");
    scenery.vertices.append(&mut commercial.vertices);
    let props = frontage
        .into_iter()
        .map(|mesh| vec![Part::baked(mesh)])
        .chain(
            groups
                .into_iter()
                .map(|group| group.into_iter().map(Part::baked).collect()),
        )
        .collect();
    Products {
        scenery: Some(scenery),
        props: Some(props),
    }
}
impl Prepared {
    pub fn take_scenery(&self, city: &City) -> Mesh {
        let mut products = self.products.borrow_mut();
        if products.scenery.is_none() {
            *products = build(city);
        }
        products.scenery.take().unwrap()
    }
    pub fn take_props(&self, city: &City, retain_scenery: bool) -> Vec<Vec<Part>> {
        let mut products = self.products.borrow_mut();
        if products.props.is_none() {
            *products = build(city);
        }
        // A packaged/disk geometry cache already owns the static product.
        if !retain_scenery {
            products.scenery = None;
        }
        products.props.take().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn city() -> City {
        City::from_program(
            &opencity::parse(
                "road local: sidewalk carlane sidewalk; end; local(-1,0,1,0);
            home=building(0.065,{use:residential},-0.3,0.08,-0.15,0.08,-0.15,0.25,-0.3,0.25);
            shop=building(0.12,{use:commercial},0.1,0.08,0.25,0.08,0.25,0.25,0.1,0.25);",
            )
            .unwrap(),
            None,
        )
        .unwrap()
    }
    fn signature(mesh: &Mesh) -> Vec<[u32; 13]> {
        mesh.vertices
            .iter()
            .map(|v| {
                let values: Vec<_> = v
                    .position
                    .into_iter()
                    .chain(v.normal)
                    .chain(v.color)
                    .chain(v.surface)
                    .map(f32::to_bits)
                    .collect();
                values.try_into().unwrap()
            })
            .collect()
    }
    #[test]
    fn startup_and_reload_consume_identical_products_without_copying() {
        let city = city();
        let (mut expected, frontage) = city.building_detail_parts_matching(
            Region::WholeMap,
            &building_style::residential(),
            detailed_frontage,
        );
        assert!(!expected.vertices.is_empty());
        assert!(!frontage.is_empty());
        expected
            .vertices
            .extend(commerce_mesh::build(&city).vertices);
        let expected_props: Vec<_> = frontage
            .into_iter()
            .map(|mesh| vec![mesh])
            .chain(commerce_mesh::prop_groups(&city))
            .collect();
        for scenery_first in [false, true] {
            let prepared = Prepared::default();
            let (scenery, props) = if scenery_first {
                let scenery = prepared.take_scenery(&city);
                let allocation = prepared.products.borrow().props.as_ref().unwrap().as_ptr();
                let props = prepared.take_props(&city, false);
                assert_eq!(allocation, props.as_ptr());
                (scenery, props)
            } else {
                let props = prepared.take_props(&city, true);
                let allocation = prepared
                    .products
                    .borrow()
                    .scenery
                    .as_ref()
                    .unwrap()
                    .vertices
                    .as_ptr();
                let scenery = prepared.take_scenery(&city);
                assert_eq!(allocation, scenery.vertices.as_ptr());
                (scenery, props)
            };
            assert_eq!(signature(&scenery), signature(&expected));
            assert_eq!(props.len(), expected_props.len());
            for (actual, expected) in props.iter().zip(&expected_props) {
                assert_eq!(actual.len(), expected.len());
                for (actual, expected) in actual.iter().zip(expected) {
                    assert_eq!(signature(&actual.mesh), signature(expected));
                }
            }
            let products = prepared.products.borrow();
            assert!(products.scenery.is_none());
            assert!(products.props.is_none());
        }
    }
    #[test]
    fn packaged_geometry_does_not_retain_unused_generated_scenery() {
        let prepared = Prepared::default();
        assert!(!prepared.take_props(&city(), false).is_empty());
        let products = prepared.products.borrow();
        assert!(products.scenery.is_none());
        assert!(products.props.is_none());
    }
}
