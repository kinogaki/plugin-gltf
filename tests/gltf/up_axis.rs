//! The up axis both ways: glTF is Y up, so a Z-up project's faces are
//! written and read back turned as a rotation turns them, never mirrored.
use crate::format::{Rec709, imported};
use kinogaki_gltf::Format;
use kinogaki_studio_api::geometry::{
    Attribute, Attributes, Domain, Geometry, Meaning, Primitives, Units, UpAxis, Vec3, Xform, names,
};
use kinogaki_studio_api::standard;
use kinogaki_studio_api::{FileFormat, FileItem, FormatOptions, Imported, Writing};
use kinogaki_studio_api_testing::format::Scratch;

/// Each placed triangle of `read` in the project: its third point, its
/// first point's normal and its normal by its winding.
fn triangle_of(read: &Imported) -> (Vec3, Vec3, Vec3) {
    let mut placed = vec![Xform::IDENTITY; read.items.len()];
    for (i, item) in read.items.iter().enumerate() {
        let parent = item.parent.map_or(Xform::IDENTITY, |p| placed[p]);
        placed[i] = parent.compose(&item.placement);
        let Some(tri) = item.data.as_ref().and_then(|d| standard::geometry().get(d)) else {
            continue;
        };
        let at = |c: usize| {
            let p = tri.positions()[tri.primitives().primitive(0)[c] as usize];
            placed[i].apply_point(p.into())
        };
        let wound = (at(1) - at(0)).cross(at(2) - at(0)).normalized().unwrap();
        let n = (tri.attributes().tuples::<[f32; 3]>(Domain::Point, names::N)).unwrap()[0];
        let n = placed[i].apply_vector(Vec3::new(n[0].into(), n[1].into(), n[2].into()));
        return (at(2), n.normalized().unwrap(), wound);
    }
    panic!("no triangle read");
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-9
}

/// glTF is Y up: a Z-up project's triangle is written turned as a rotation
/// does (its +Z the file's +Y, its +Y the file's −Z), and read back into a
/// Z-up project turned the other way, facing where its normals point both
/// times.
#[test]
fn a_z_up_projects_faces_write_and_read_back_turned_not_mirrored() {
    let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let table = Primitives::of_polygons(3, vec![0, 3].into(), vec![0, 1, 2].into()).unwrap();
    let n = Attribute::tuples(vec![[0.0f32, 0.0, 1.0]; 3], Meaning::Normal).unwrap();
    let attributes = Attributes::new().with(Domain::Point, names::N, n).unwrap();
    let tri = Geometry::new(positions.into(), table, attributes).unwrap();
    let z_up = Units::new(1.0, UpAxis::Z).unwrap();
    let item = FileItem::top("Tri", standard::geometry().wrap(tri));
    let dir = Scratch::new("turned");
    let path = dir.join("tri.glb");
    let writer = Format.writer().unwrap();
    (writer.write(
        &path,
        &[item],
        &Writing::new(z_up, &Rec709),
        &FormatOptions::new(),
    ))
    .unwrap();
    // As the file holds it: Y up.
    let (p, n, wound) = triangle_of(&imported(&path, Units::default()));
    assert!(
        near(p, Vec3::new(0.0, 0.0, -1.0)),
        "+Y is the file's −Z: {p:?}"
    );
    assert!(near(n, Vec3::Y), "+Z is the file's +Y: {n:?}");
    assert!(near(wound, Vec3::Y), "it faces its normal: {wound:?}");
    // Back in a Z-up project: where it was.
    let (p, n, wound) = triangle_of(&imported(&path, z_up));
    assert!(near(p, Vec3::Y), "{p:?}");
    assert!(near(n, Vec3::Z), "{n:?}");
    assert!(near(wound, Vec3::Z), "it faces its normal: {wound:?}");
}
