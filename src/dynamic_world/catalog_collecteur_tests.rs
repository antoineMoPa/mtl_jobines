#[test]
fn collecteur_retains_original_texture_symmetric_body_and_dual_rear_wheels() {
    super::vehicle_qa::textured_truck(
        include_bytes!("../../data/vehicle/municipal/castor-collecteur/model.glb"),
        "vehicle-j05",
        4,
        12,
        1_328_800,
    );
}
