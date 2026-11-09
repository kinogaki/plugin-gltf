//! File ▸ Open's read: a glTF in its own space, metres and Y up, its items
//! as Import reads them into those units.
use crate::common;
use kinogaki_gltf::Format;
use kinogaki_studio_api::geometry::Units;
use kinogaki_studio_api::{FileFormat, Input};
use kinogaki_studio_api_testing::format::document;

#[test]
fn a_gltf_opens_in_metres_y_up() {
    for (model, variant) in [
        ("BoxTextured", "glTF"),
        ("Cameras", "glTF-Embedded"),
        ("LightsPunctualLamp", "glTF-Binary"),
    ] {
        let path = common::sample(model, variant);
        let units = document(&Format, &Input::file(&path), &Format.options());
        assert_eq!(units, Ok(Units::METERS), "{model}");
    }
}
