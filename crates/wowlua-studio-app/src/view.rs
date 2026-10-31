//! Studio's own view of the 3D world (S088): a camera that circles the
//! character or looks straight down on it, and where a point of the world
//! shows on the screen as the game's renderer draws it.

use crate::system::{ViewCamera, WorldSeen};

/// How high over its feet the orbit circles the character (yards).
const CIRCLED: f64 = 1.5;
/// The near plane of the world's projection (yards; the renderer's).
const NEAR: f64 = 0.2;

/// The eye, the way it looks and its up (north, west, up yards) of a view
/// of the character standing at `player`; none for the game's camera.
pub fn camera(view: ViewCamera, player: [f64; 3]) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    match view {
        ViewCamera::Follow => None,
        ViewCamera::Orbit {
            yaw,
            pitch,
            distance,
        } => {
            let target = [player[0], player[1], player[2] + CIRCLED];
            let out = [
                pitch.cos() * yaw.cos(),
                pitch.cos() * yaw.sin(),
                pitch.sin(),
            ];
            let eye = [
                target[0] + out[0] * distance,
                target[1] + out[1] * distance,
                target[2] + out[2] * distance,
            ];
            Some((eye, [-out[0], -out[1], -out[2]], [0.0, 0.0, 1.0]))
        }
        // (straight down, north at the top of the screen)
        ViewCamera::Above { height } => Some((
            [player[0], player[1], player[2] + height],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
        )),
    }
}

/// Where a point of the world shows on a screen `width` by `height` (top
/// left origin) as the world was drawn: as `wowlua_render::project`; none
/// behind the eye.
pub fn project(w: &WorldSeen, width: f32, height: f32, p: [f64; 3]) -> Option<[f32; 2]> {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let unit = |a: [f64; 3]| {
        let l = dot(a, a).sqrt().max(1e-9);
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let f = unit(w.look);
    let r = unit(cross(f, w.up));
    let u = cross(r, f);
    let d = [p[0] - w.eye[0], p[1] - w.eye[1], p[2] - w.eye[2]];
    let depth = dot(d, f);
    if depth <= NEAR * 0.5 {
        return None;
    }
    let aspect = f64::from(width) / f64::from(height.max(1.0));
    let tan_x = (w.fov / 2.0).tan();
    let tan_y = tan_x / aspect;
    let x = dot(d, r) / (depth * tan_x);
    let y = dot(d, u) / (depth * tan_y);
    Some([
        ((x + 1.0) / 2.0 * f64::from(width)) as f32,
        ((1.0 - y) / 2.0 * f64::from(height)) as f32,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn north() -> WorldSeen {
        WorldSeen {
            eye: [0.0; 3],
            look: [1.0, 0.0, 0.0],
            up: [0.0, 0.0, 1.0],
            fov: 90f64.to_radians(),
            far: 1000.0,
            units: Vec::new(),
        }
    }

    #[test]
    fn a_point_shows_where_the_camera_sees_it() {
        let w = north();
        let ahead = project(&w, 1000.0, 500.0, [10.0, 0.0, 0.0]).unwrap();
        assert!((ahead[0] - 500.0).abs() < 0.5 && (ahead[1] - 250.0).abs() < 0.5);
        let west = project(&w, 1000.0, 500.0, [10.0, 10.0, 0.0]).unwrap();
        assert!(west[0].abs() < 0.5, "{west:?}");
        assert!(project(&w, 1000.0, 500.0, [-10.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn the_views_look_at_the_character() {
        let at = [100.0, 50.0, 10.0];
        assert!(camera(ViewCamera::Follow, at).is_none());
        let (eye, look, up) = camera(ViewCamera::Above { height: 40.0 }, at).unwrap();
        assert_eq!(
            (eye, look, up),
            ([100.0, 50.0, 50.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0])
        );
        let (eye, look, _) = camera(
            ViewCamera::Orbit {
                yaw: 0.0,
                pitch: 0.0,
                distance: 10.0,
            },
            at,
        )
        .unwrap();
        // (north of it, level with its head, looking south)
        assert_eq!(eye, [110.0, 50.0, 11.5]);
        assert_eq!(look, [-1.0, -0.0, -0.0]);
    }
}
