//! Skeletons, rigid voxel skinning and keyframe animation.

use std::f32::consts::TAU;

use super::math::{Affine, Vec3};
use super::model::{Voxel, VoxelModel};

/// A joint. Voxels bound to a bone follow it rigidly; `head` is the pivot, `tail` the end
/// (used for automatic binding and for drawing the skeleton).
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub head: Vec3,
    pub tail: Vec3,
}

/// Local bone pose at a frame: Euler rotation (degrees, around the head) and translation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub bone: usize,
    pub frame: f32,
    pub rot: Vec3,
    pub pos: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub name: String,
    pub frames: u32,
    pub frame_ms: u32,
    pub looped: bool,
    pub keys: Vec<Key>,
}

impl Animation {
    pub fn new(name: &str, frames: u32, frame_ms: u32) -> Animation {
        Animation { name: name.into(), frames: frames.max(1), frame_ms, looped: true, keys: Vec::new() }
    }

    /// Inserts or replaces the key of `bone` at `frame`.
    pub fn set_key(&mut self, key: Key) {
        self.keys.retain(|k| !(k.bone == key.bone && (k.frame - key.frame).abs() < 1e-4));
        self.keys.push(key);
        self.keys.sort_by(|a, b| a.bone.cmp(&b.bone).then(a.frame.total_cmp(&b.frame)));
    }

    /// Interpolated (rotation, translation) of a bone at time `t` (in frames).
    pub fn sample(&self, bone: usize, t: f32) -> (Vec3, Vec3) {
        let keys: Vec<&Key> = self.keys.iter().filter(|k| k.bone == bone).collect();
        match keys.len() {
            0 => (Vec3::ZERO, Vec3::ZERO),
            1 => (keys[0].rot, keys[0].pos),
            _ => {
                let len = self.frames as f32;
                let t = if self.looped { t.rem_euclid(len) } else { t.clamp(0.0, len) };
                let next = keys.iter().position(|k| k.frame > t);
                let (k0, k1, span) = match next {
                    Some(0) => {
                        let last = keys[keys.len() - 1];
                        if self.looped { (last, keys[0], keys[0].frame + len - last.frame) } else { return (keys[0].rot, keys[0].pos) }
                    }
                    Some(i) => (keys[i - 1], keys[i], keys[i].frame - keys[i - 1].frame),
                    None => {
                        let last = keys[keys.len() - 1];
                        if self.looped { (last, keys[0], keys[0].frame + len - last.frame) } else { return (last.rot, last.pos) }
                    }
                };
                let local = (t - k0.frame).rem_euclid(len.max(1.0));
                let u = if span <= 1e-6 { 0.0 } else { (local / span).clamp(0.0, 1.0) };
                let s = u * u * (3.0 - 2.0 * u);
                (k0.rot.lerp(k1.rot, s), k0.pos.lerp(k1.pos, s))
            }
        }
    }
}

/// World transforms of every bone for an animation time (rest pose when `anim` is None).
pub fn pose(bones: &[Bone], anim: Option<&Animation>, t: f32) -> Vec<Affine> {
    let mut world: Vec<Affine> = Vec::with_capacity(bones.len());
    for (i, b) in bones.iter().enumerate() {
        let (rot, pos) = anim.map(|a| a.sample(i, t)).unwrap_or((Vec3::ZERO, Vec3::ZERO));
        let local = Affine::translation(pos).then(&Affine::rotation_about(b.head, rot));
        let w = match b.parent {
            Some(p) if p < i => world[p].then(&local),
            _ => local,
        };
        world.push(w);
    }
    world
}

/// Distance from a point to a bone segment.
fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    let t = if len2 < 1e-6 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
    (p - (a + ab * t)).length()
}

/// Binds voxels to their nearest bone segment. `only_unbound` keeps explicit bindings.
pub fn auto_bind(model: &mut VoxelModel, only_unbound: bool) -> usize {
    if model.bones.is_empty() {
        return 0;
    }
    let bones = model.bones.clone();
    let mut changes: Vec<(i32, i32, i32, Voxel)> = Vec::new();
    model.for_each(|x, y, z, v| {
        if only_unbound && v.bone != 0 {
            return;
        }
        let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
        let mut best = (f32::MAX, 0usize);
        for (i, b) in bones.iter().enumerate() {
            let d = segment_distance(p, b.head, b.tail);
            if d < best.0 {
                best = (d, i);
            }
        }
        if v.bone as usize != best.1 {
            changes.push((x, y, z, Voxel { bone: best.1 as u8, ..v }));
        }
    });
    let n = changes.len();
    for (x, y, z, v) in changes {
        model.set(x, y, z, v);
    }
    n
}

/// Procedural humanoid motion cycles (walk, run, idle) driven by standard bone names:
/// hips, spine, chest, neck, head, upper_arm_l/r, lower_arm_l/r, hand_l/r,
/// upper_leg_l/r, lower_leg_l/r, foot_l/r, and optional cape, weapon. Missing bones are skipped.
pub struct CycleParams {
    pub frames: u32,
    pub frame_ms: u32,
    pub stride_deg: f32,
    pub knee_deg: f32,
    pub arm_swing_deg: f32,
    pub bob: f32,
    pub lean_deg: f32,
}

impl CycleParams {
    pub fn walk(frames: u32) -> CycleParams {
        CycleParams { frames, frame_ms: 100, stride_deg: 28.0, knee_deg: 38.0, arm_swing_deg: 24.0, bob: 2.0, lean_deg: 3.0 }
    }
    pub fn run(frames: u32) -> CycleParams {
        CycleParams { frames, frame_ms: 70, stride_deg: 42.0, knee_deg: 75.0, arm_swing_deg: 40.0, bob: 4.0, lean_deg: 10.0 }
    }
}

pub fn locomotion_cycle(model: &VoxelModel, name: &str, p: &CycleParams) -> Animation {
    let mut a = Animation::new(name, p.frames, p.frame_ms);
    let bone = |n: &str| model.bone_index(n);
    let n = p.frames.max(2);
    let length = |n: &str| bone(n).map(|i| (model.bones[i].tail - model.bones[i].head).length()).unwrap_or(0.0);
    let (thigh, shin) = (length("upper_leg_l").max(length("upper_leg_r")), length("lower_leg_l").max(length("lower_leg_r")));
    // Vertical reach of a leg for hip angle `h` and knee bend `k` (degrees).
    let reach = |h: f32, k: f32| thigh * h.to_radians().cos() + shin * (h + k).to_radians().cos();
    let rest = reach(0.0, 0.0);
    for f in 0..n {
        let phi = TAU * f as f32 / n as f32;
        let (s, c) = phi.sin_cos();
        let mut key = |b: Option<usize>, rot: Vec3, pos: Vec3| {
            if let Some(bone) = b {
                a.set_key(Key { bone, frame: f as f32, rot, pos });
            }
        };
        // Character faces +Z: negative X rotation swings a hanging limb forward.
        let leg_l = -p.stride_deg * s;
        let leg_r = p.stride_deg * s;
        let knee_l = 6.0 + p.knee_deg * c.max(0.0).powf(1.5);
        let knee_r = 6.0 + p.knee_deg * (-c).max(0.0).powf(1.5);
        // Foot planting: lower the hips so the longer (stance) leg still reaches the ground.
        let plant = rest - reach(leg_l, knee_l).max(reach(leg_r, knee_r));
        key(bone("hips"), Vec3::new(0.0, 6.0 * s, 2.0 * c), Vec3::new(0.0, -plant + p.bob * 0.5 * ((2.0 * phi).cos() - 1.0), 0.0));
        key(bone("spine"), Vec3::new(p.lean_deg, -3.0 * s, 0.0), Vec3::ZERO);
        key(bone("chest"), Vec3::new(1.5 * (2.0 * phi).cos(), -6.0 * s, -1.5 * c), Vec3::ZERO);
        key(bone("neck"), Vec3::new(-p.lean_deg * 0.5, 2.0 * s, 0.0), Vec3::ZERO);
        key(bone("head"), Vec3::new(-1.5 * (2.0 * phi).cos(), 3.0 * s, 0.0), Vec3::ZERO);
        key(bone("upper_leg_l"), Vec3::new(leg_l, 0.0, 0.0), Vec3::ZERO);
        key(bone("upper_leg_r"), Vec3::new(leg_r, 0.0, 0.0), Vec3::ZERO);
        key(bone("lower_leg_l"), Vec3::new(knee_l, 0.0, 0.0), Vec3::ZERO);
        key(bone("lower_leg_r"), Vec3::new(knee_r, 0.0, 0.0), Vec3::ZERO);
        key(bone("foot_l"), Vec3::new(-(leg_l + knee_l) * 0.75, 0.0, 0.0), Vec3::ZERO);
        key(bone("foot_r"), Vec3::new(-(leg_r + knee_r) * 0.75, 0.0, 0.0), Vec3::ZERO);
        // Character's left is +X; positive Z rotation opens the left arm away from the body.
        key(bone("upper_arm_l"), Vec3::new(p.arm_swing_deg * s, 0.0, 5.0), Vec3::ZERO);
        key(bone("upper_arm_r"), Vec3::new(-p.arm_swing_deg * s, 0.0, -5.0), Vec3::ZERO);
        key(bone("lower_arm_l"), Vec3::new(-14.0 - 12.0 * s.max(0.0), 0.0, 0.0), Vec3::ZERO);
        key(bone("lower_arm_r"), Vec3::new(-14.0 - 12.0 * (-s).max(0.0), 0.0, 0.0), Vec3::ZERO);
        key(bone("cape"), Vec3::new(8.0 + 6.0 * (1.0 - (2.0 * phi).cos()) * 0.5 + p.lean_deg, 0.0, 3.0 * s), Vec3::ZERO);
        key(bone("weapon"), Vec3::new(-3.0 * s, 0.0, 0.0), Vec3::ZERO);
    }
    a
}

pub fn idle_cycle(model: &VoxelModel, name: &str, frames: u32) -> Animation {
    let mut a = Animation::new(name, frames, 160);
    let n = frames.max(2);
    for f in 0..n {
        let phi = TAU * f as f32 / n as f32;
        let s = phi.sin();
        for (bone, rot, pos) in [
            ("hips", Vec3::ZERO, Vec3::new(0.0, -0.6 * (1.0 - phi.cos()) * 0.5, 0.0)),
            ("chest", Vec3::new(-1.5 * s, 0.0, 0.0), Vec3::ZERO),
            ("head", Vec3::new(1.0 * s, 1.5 * s, 0.0), Vec3::ZERO),
            ("upper_arm_l", Vec3::new(2.0 * s, 0.0, 6.0 + 1.5 * s), Vec3::ZERO),
            ("upper_arm_r", Vec3::new(2.0 * s, 0.0, -6.0 - 1.5 * s), Vec3::ZERO),
            ("lower_arm_l", Vec3::new(-10.0, 0.0, 0.0), Vec3::ZERO),
            ("lower_arm_r", Vec3::new(-10.0, 0.0, 0.0), Vec3::ZERO),
            ("cape", Vec3::new(4.0 + 2.0 * s, 0.0, 1.5 * phi.cos()), Vec3::ZERO),
        ] {
            if let Some(b) = model.bone_index(bone) {
                a.set_key(Key { bone: b, frame: f as f32, rot, pos });
            }
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_bone_model() -> VoxelModel {
        let mut m = VoxelModel::new("arm", [32, 32, 32]);
        m.bones.push(Bone { name: "upper".into(), parent: None, head: Vec3::new(16.0, 30.0, 16.0), tail: Vec3::new(16.0, 20.0, 16.0) });
        m.bones.push(Bone { name: "lower".into(), parent: Some(0), head: Vec3::new(16.0, 20.0, 16.0), tail: Vec3::new(16.0, 10.0, 16.0) });
        m
    }

    #[test]
    fn interpolation_wraps() {
        let mut a = Animation::new("a", 8, 100);
        a.set_key(Key { bone: 0, frame: 0.0, rot: Vec3::ZERO, pos: Vec3::ZERO });
        a.set_key(Key { bone: 0, frame: 4.0, rot: Vec3::new(40.0, 0.0, 0.0), pos: Vec3::ZERO });
        assert_eq!(a.sample(0, 4.0).0.x, 40.0);
        assert!((a.sample(0, 2.0).0.x - 20.0).abs() < 1e-4);
        assert!((a.sample(0, 6.0).0.x - 20.0).abs() < 1e-4, "wraps from frame 4 back to 0");
        assert_eq!(a.sample(1, 3.0).0, Vec3::ZERO);
    }

    #[test]
    fn hierarchy_and_binding() {
        let mut m = two_bone_model();
        m.set(16, 25, 16, Voxel { rgba: 0xFFFFFFFF, ..Voxel::EMPTY });
        m.set(16, 12, 16, Voxel { rgba: 0xFFFFFFFF, ..Voxel::EMPTY });
        assert_eq!(auto_bind(&mut m, false), 1);
        assert_eq!(m.get(16, 12, 16).bone, 1);
        let mut a = Animation::new("bend", 1, 100);
        a.set_key(Key { bone: 0, frame: 0.0, rot: Vec3::new(0.0, 0.0, 90.0), pos: Vec3::ZERO });
        let w = pose(&m.bones, Some(&a), 0.0);
        // The child follows the parent's rotation around the shoulder.
        let tip = w[1].apply(Vec3::new(16.0, 10.0, 16.0));
        assert!((tip.x - 36.0).abs() < 1e-3 && (tip.y - 30.0).abs() < 1e-3, "{tip:?}");
    }

    #[test]
    fn walk_cycle_has_keys_for_existing_bones() {
        let mut m = VoxelModel::new("h", [64, 64, 64]);
        for name in ["hips", "upper_leg_l", "upper_leg_r"] {
            m.bones.push(Bone { name: name.into(), parent: None, head: Vec3::ZERO, tail: Vec3::ZERO });
        }
        let a = locomotion_cycle(&m, "walk", &CycleParams::walk(8));
        assert_eq!(a.frames, 8);
        assert_eq!(a.keys.len(), 3 * 8);
        let l = a.sample(1, 2.0).0.x;
        let r = a.sample(2, 2.0).0.x;
        assert!((l + r).abs() < 1e-3 && l.abs() > 10.0, "legs swing in opposition");
    }
}
