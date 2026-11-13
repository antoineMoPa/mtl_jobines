#[test]
fn snowplow_retains_original_texture_symmetric_body_and_recessed_wheels() {
    super::vehicle_qa::textured_truck(
        include_bytes!("../../data/vehicle/municipal/castor-blizzard/model.glb"),
        "vehicle-j03",
        6,
        13,
        1_248_356,
    );
}
