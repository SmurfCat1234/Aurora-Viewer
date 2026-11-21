use glam::{Mat3, Quat, Vec3};

use super::Frame;
use crate::scene::anim::Rig;

const SIDES: [&str; 2] = ["Left", "Right"];
const FINGERS: [(&str, usize); 5] = [("Thumb", 1), ("Index", 5), ("Middle", 9), ("Ring", 13), ("Pinky", 17)];

#[derive(Clone, Copy)]
pub(super) struct Channels {
    pub head: bool,
    pub eyes: bool,
    pub expression: bool,
    pub hands: bool,
    pub upper_body: bool,
    pub expression_strength: f32,
}

impl Default for Channels {
    fn default() -> Self {
        Self {
            head: true,
            eyes: true,
            expression: true,
            hands: true,
            upper_body: true,
            expression_strength: 1.0,
        }
    }
}

#[derive(Clone, Copy)]
struct Face {
    head: Quat,
    jaw: f32,
    openness: [f32; 2],
    gaze: [[f32; 2]; 2],
}

impl Face {
    fn measure(points: [[f32; 3]; 16]) -> Option<Self> {
        let p = points.map(Vec3::from_array);
        let left = p[0] - p[1];
        let up = p[2] - p[3];
        let head = basis(left.cross(up), left)?;
        let lip_width = p[6].distance(p[7]).max(0.001);
        let eye = |outer: Vec3, inner: Vec3, upper: Vec3, lower: Vec3, iris: Vec3| {
            let width = outer.distance(inner).max(0.001);
            let offset = head.inverse() * (iris - (outer + inner) * 0.5) / width;
            (upper.distance(lower) / width, [offset.y, offset.z])
        };
        let (left_open, left_gaze) = eye(p[0], p[8], p[9], p[10], p[11]);
        let (right_open, right_gaze) = eye(p[1], p[12], p[13], p[14], p[15]);
        Some(Self {
            head,
            jaw: p[4].distance(p[5]) / lip_width,
            openness: [left_open, right_open],
            gaze: [left_gaze, right_gaze],
        })
    }
}

pub(crate) struct Pose {
    body: [Option<Vec3>; 8],
    hands: [Option<[[f32; 3]; 21]>; 2],
    face: Option<(Face, Face)>,
    chest: Option<Quat>,
    strength: f32,
    channels: Channels,
}

impl Pose {
    pub(super) fn new(frame: &Frame, neutral: Option<&Frame>, strength: f32, channels: Channels) -> Self {
        let neutral_face = neutral.and_then(|f| Face::measure(f.face?));
        let face = frame.face.and_then(Face::measure).zip(neutral_face);
        let body = body_points(frame);
        let chest = torso(&body)
            .zip(neutral.and_then(|f| torso(&body_points(f))))
            .map(|(current, rest)| limited(current * rest.inverse(), 0.48));
        Self {
            body,
            hands: [frame.left, frame.right],
            face,
            chest,
            strength,
            channels,
        }
    }

    pub(crate) fn rotations(&self, rig: &Rig, animated: &[Quat]) -> Vec<Quat> {
        let mut targets = vec![None; rig.local_rot.len()];
        if self.channels.upper_body
            && let Some(chest) = self.chest
        {
            set(rig, &mut targets, "mChest", Target::Local(chest));
        }
        if let Some((face, neutral)) = self.face {
            self.face_targets(rig, &mut targets, face, neutral);
        }
        for (i, side) in SIDES.iter().enumerate() {
            if self.channels.upper_body {
                self.arm_targets(rig, &mut targets, i, side);
            }
            if self.channels.hands {
                self.hand_targets(rig, &mut targets, i, side);
            }
        }

        let mut output = animated.to_vec();
        let mut world = vec![Quat::IDENTITY; rig.local_rot.len()];
        for &joint in &rig.order {
            let parent = rig.parent[joint].map_or(Quat::IDENTITY, |p| world[p]);
            if let Some(target) = targets[joint] {
                let rest = rig.local_rot[joint];
                let local = match target {
                    Target::Local(delta) => rest * delta,
                    Target::Absolute(goal) => parent.inverse() * goal,
                    Target::Direction { from, to } => {
                        let from = rest * from;
                        let to = parent.inverse() * to;
                        Quat::from_rotation_arc(from, to) * rest
                    }
                };
                output[joint] = output[joint].slerp(local.normalize(), self.strength);
            }
            world[joint] = parent * output[joint];
        }
        output
    }

    fn face_targets(&self, rig: &Rig, targets: &mut [Option<Target>], face: Face, neutral: Face) {
        if self.channels.head {
            let head = limited(face.head * neutral.head.inverse(), 1.05);
            set(rig, targets, "mHead", Target::Absolute(head));
        }
        let gain = self.channels.expression_strength;
        let expression = self.channels.expression && gain > 0.0;
        if expression {
            let jaw = ((face.jaw - neutral.jaw - 0.015) * 1.55 * gain).clamp(0.0, 0.56);
            set(rig, targets, "mFaceJaw", Target::Local(Quat::from_rotation_y(jaw)));
        }
        for (i, side) in SIDES.iter().enumerate() {
            if self.channels.eyes {
                let yaw = (face.gaze[i][0] - neutral.gaze[i][0]).clamp(-0.23, 0.23);
                let pitch = (neutral.gaze[i][1] - face.gaze[i][1]).clamp(-0.15, 0.15);
                let eye = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch);
                for joint in ["mEye", "mFaceEyeAlt"] {
                    set(rig, targets, &format!("{joint}{side}"), Target::Local(eye));
                }
            }
            if expression {
                let blink = ((1.0 - face.openness[i] / neutral.openness[i].max(0.1)) * gain).clamp(0.0, 1.0);
                for (joint, angle) in [("mFaceEyeLidUpper", blink * 0.48), ("mFaceEyeLidLower", -blink * 0.14)] {
                    set(rig, targets, &format!("{joint}{side}"), Target::Local(Quat::from_rotation_y(angle)));
                }
            }
        }
    }

    fn arm_targets(&self, rig: &Rig, targets: &mut [Option<Target>], i: usize, side: &str) {
        let [shoulder, elbow, wrist] = [self.body[i * 3], self.body[i * 3 + 1], self.body[i * 3 + 2]];
        for (joint, child, start, end) in [("Shoulder", "Elbow", shoulder, elbow), ("Elbow", "Wrist", elbow, wrist)] {
            let child = format!("m{child}{side}");
            if let Some((start, end)) = start.zip(end)
                && let Some(to) = (end - start).try_normalize()
                && let Some(&child) = rig.names.get(&child)
                && let Some(from) = rig.local_pos[child].try_normalize()
            {
                set(rig, targets, &format!("m{joint}{side}"), Target::Direction { from, to });
            }
        }
    }

    fn hand_targets(&self, rig: &Rig, targets: &mut [Option<Target>], i: usize, side: &str) {
        let Some(hand) = self.hands[i] else { return };
        let hand = hand.map(Vec3::from_array);
        let index = format!("mHandIndex1{side}");
        let pinky = format!("mHandPinky1{side}");
        if let Some((&index, &pinky)) = rig.names.get(&index).zip(rig.names.get(&pinky)) {
            let index = rig.local_pos[index];
            let pinky = rig.local_pos[pinky];
            if let Some((tracked, rest)) =
                basis(hand[5] - hand[17], (hand[5] + hand[17]) * 0.5 - hand[0]).zip(basis(index - pinky, (index + pinky) * 0.5))
            {
                set(rig, targets, &format!("mWrist{side}"), Target::Absolute(tracked * rest.inverse()));
            }
        }
        for (finger, start) in FINGERS {
            let mut previous = hand[start] - hand[0];
            for joint in 0..3 {
                let link = hand[start + joint + 1] - hand[start + joint];
                if let Some((a, b)) = previous.try_normalize().zip(link.try_normalize()) {
                    let curl = (a.angle_between(b) - 0.16).clamp(0.0, 1.55);
                    let sign = if i == 0 { -1.0 } else { 1.0 };
                    let axis = if finger == "Thumb" { Vec3::Z * -sign } else { Vec3::X * sign };
                    let delta = Quat::from_axis_angle(axis, curl);
                    set(rig, targets, &format!("mHand{finger}{}{side}", joint + 1), Target::Local(delta));
                }
                previous = link;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Target {
    Local(Quat),
    Absolute(Quat),
    Direction { from: Vec3, to: Vec3 },
}

fn set(rig: &Rig, targets: &mut [Option<Target>], joint: &str, target: Target) {
    if let Some(&joint) = rig.names.get(joint) {
        targets[joint] = Some(target);
    }
}

fn body_points(frame: &Frame) -> [Option<Vec3>; 8] {
    frame.body.map(|point| point.map(Vec3::from_array))
}

fn torso(body: &[Option<Vec3>; 8]) -> Option<Quat> {
    let left = body[0]?;
    let right = body[3]?;
    let up = body[6].zip(body[7]).map_or(Vec3::Z, |(a, b)| (left + right - a - b) * 0.5);
    let across = left - right;
    basis(across.cross(up), across)
}

fn basis(x: Vec3, y: Vec3) -> Option<Quat> {
    let x = x.try_normalize()?;
    let z = x.cross(y).try_normalize()?;
    let y = z.cross(x);
    Some(Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize())
}

fn limited(rotation: Quat, limit: f32) -> Quat {
    let rotation = if rotation.w < 0.0 { -rotation } else { rotation };
    let (axis, angle) = rotation.to_axis_angle();
    Quat::from_axis_angle(axis, angle.min(limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_is_right_handed_and_rejects_collinear_input() {
        assert!(basis(Vec3::X, Vec3::Y).unwrap().abs_diff_eq(Quat::IDENTITY, 1e-5));
        assert!(basis(Vec3::X, Vec3::X).is_none());
        let turned = basis(Vec3::Y, -Vec3::X).unwrap();
        assert!((turned * Vec3::X).abs_diff_eq(Vec3::Y, 1e-5));
    }

    #[test]
    fn rotation_limit_takes_the_shortest_arc() {
        let turn = -Quat::from_rotation_z(0.8);
        assert!(limited(turn, 0.3).abs_diff_eq(Quat::from_rotation_z(0.3), 1e-5));
    }

    #[test]
    fn tracked_arms_use_parent_space_and_leave_legs_animated() {
        let xml = include_str!("../../assets/character/avatar_skeleton.xml");
        let rig = Rig::new(&aurora_assets::Skeleton::parse(xml.as_bytes()).unwrap());
        let mut frame = Frame::default();
        frame.body[0] = Some([0.0, 0.2, 0.0]);
        frame.body[1] = Some([0.0, 0.3, -0.2]);
        frame.body[2] = Some([0.1, 0.3, 0.1]);
        let mut animated = rig.local_rot.clone();
        let collar = rig.names["mCollarLeft"];
        let knee = rig.names["mKneeLeft"];
        animated[collar] = Quat::from_rotation_z(0.2);
        animated[knee] = Quat::from_rotation_y(0.4);
        let shoulder = rig.names["mShoulderLeft"];
        let elbow = rig.names["mElbowLeft"];
        let wrist = rig.names["mWristLeft"];
        let pose = Pose::new(&frame, None, 1.0, Channels::default()).rotations(&rig, &animated);
        let upper = pose[collar] * pose[shoulder];
        let lower = upper * pose[elbow];
        let up_dir = Vec3::new(0.0, 0.1, -0.2).normalize();
        let low_dir = Vec3::new(0.1, 0.0, 0.3).normalize();
        assert!((upper * rig.local_pos[elbow].normalize()).abs_diff_eq(up_dir, 1e-4));
        assert!((lower * rig.local_pos[wrist].normalize()).abs_diff_eq(low_dir, 1e-4));
        assert_eq!(pose[knee], animated[knee]);
        let released = Pose::new(&frame, None, 0.0, Channels::default()).rotations(&rig, &animated);
        assert!(released[shoulder].abs_diff_eq(animated[shoulder], 1e-5));
        let disabled = Channels {
            upper_body: false,
            ..Channels::default()
        };
        let disabled = Pose::new(&frame, None, 1.0, disabled).rotations(&rig, &animated);
        assert_eq!(disabled[shoulder], animated[shoulder]);
    }
}
