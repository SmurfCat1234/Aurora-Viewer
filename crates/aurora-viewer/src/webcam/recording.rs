//! Capture only webcam-authored joint rotations and bake native animation assets.

use super::Pose;
use crate::{
    ik_edit::{PreparedAnimation, write_animation_atomic},
    scene::anim::{Rig, WebcamFeet},
    theme::Palette,
    ui::widgets,
};
use aurora_assets::anim::{Animation, JointMotion, RotKey};
use aurora_i18n::tr;
use glam::Quat;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

const INTERVAL: f32 = 1.0 / 30.0;
const ASSET_BYTES: usize = 250_000;

struct Sample {
    time: f32,
    rotations: Vec<Option<Quat>>,
}

struct Capture {
    starts: Instant,
    limit: f32,
    next_sample: f32,
    names: Vec<String>,
    samples: Vec<Sample>,
    feet: WebcamFeet,
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    Empty,
    Size,
    Write,
}

enum Output {
    Saved,
    Upload(PreparedAnimation),
    Cancelled,
}

pub(super) struct Recorder {
    active: Option<Capture>,
    clip: Option<Arc<Animation>>,
    limit: u32,
    countdown: bool,
    range: [f32; 2],
    cursor: f32,
    visible: bool,
    playing: Option<(Instant, f32)>,
    looping: bool,
    job: Option<mpsc::Receiver<Result<Output, Failure>>>,
    saved: bool,
    failure: Option<Failure>,
    upload: Option<PreparedAnimation>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            active: None,
            clip: None,
            limit: 30,
            countdown: true,
            range: [0.0; 2],
            cursor: 0.0,
            visible: false,
            playing: None,
            looping: false,
            job: None,
            saved: false,
            failure: None,
            upload: None,
        }
    }
}

impl Recorder {
    pub(super) fn hide_preview(&mut self) {
        self.visible = false;
        self.playing = None;
    }

    pub(super) fn take_upload(&mut self) -> Option<PreparedAnimation> {
        self.poll();
        self.upload.take()
    }

    fn start(&mut self, now: Instant) {
        self.hide_preview();
        self.failure = None;
        self.saved = false;
        let delay = if self.countdown { Duration::from_secs(3) } else { Duration::ZERO };
        self.active = Some(Capture {
            starts: now + delay,
            limit: self.limit as f32,
            next_sample: 0.0,
            names: Vec::new(),
            samples: Vec::new(),
            feet: WebcamFeet::default(),
        });
    }

    pub(super) fn tick(&mut self, rig: &Rig, now: Instant, pose: Option<&Pose>) {
        let Some(capture) = &mut self.active else { return };
        let Some(elapsed) = now.checked_duration_since(capture.starts) else {
            return;
        };
        let elapsed = elapsed.as_secs_f32();
        let time = elapsed.min(capture.limit);
        if time + 0.001 >= capture.next_sample {
            if let Some(pose) = pose {
                if capture.names.is_empty() {
                    capture.names = rig.canonical_names.clone();
                }
                capture.samples.push(Sample {
                    time,
                    rotations: pose.capture(rig, &mut capture.feet),
                });
            }
            // Keep real elapsed time across a slow render or a dropped camera frame.
            capture.next_sample = (1.0 + ((time + 0.001) / INTERVAL).floor()) * INTERVAL;
        }
        if elapsed >= capture.limit {
            self.finish();
        }
    }

    pub(super) fn finish(&mut self) {
        let Some(capture) = self.active.take() else { return };
        if capture.samples.is_empty() {
            if Instant::now() >= capture.starts {
                self.failure = Some(Failure::Empty);
            }
            return;
        }
        let Some(clip) = assemble(capture.samples, capture.names) else {
            self.failure = Some(Failure::Empty);
            return;
        };
        self.range = [0.0, clip.duration];
        self.cursor = 0.0;
        self.clip = Some(Arc::new(clip));
    }

    pub(super) fn preview(&mut self, rig: &Rig, now: Instant) -> Option<Arc<Pose>> {
        if !self.visible {
            return None;
        }
        let clip = self.clip.as_ref()?;
        if let Some((start, time)) = self.playing {
            let time = time + now.saturating_duration_since(start).as_secs_f32();
            let [a, b] = self.range;
            if self.looping && b > a {
                self.cursor = a + (time - a).rem_euclid(b - a);
            } else {
                self.cursor = time.min(b);
                if time >= b {
                    self.playing = None;
                }
            }
        }
        let mut joints = vec![None; rig.len()];
        for joint in &clip.joints {
            if let Some(&index) = rig.names.get(&joint.joint_name) {
                joints[index] = joint.rotation_at(self.cursor);
            }
        }
        Some(Arc::new(Pose::recorded(joints)))
    }

    fn export(&mut self, for_upload: bool, ctx: &egui::Context) {
        let Some(clip) = self.clip.clone() else { return };
        let range = self.range;
        let looping = self.looping;
        self.failure = None;
        self.saved = false;
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        let title = tr!("webcam-record-save").to_string();
        let name = tr!("webcam-record-name").to_string();
        let spawned = std::thread::Builder::new().name("webcam-animation-export".into()).spawn(move || {
            let result = prepare(&clip, range, looping, name.clone()).and_then(|prepared| {
                if for_upload {
                    return Ok(Output::Upload(prepared));
                }
                let Some(mut path) = rfd::FileDialog::new()
                    .set_title(title)
                    .set_file_name(format!("{name}.anim"))
                    .add_filter("SL .anim", &["anim"])
                    .save_file()
                else {
                    return Ok(Output::Cancelled);
                };
                if !path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("anim"))
                {
                    path.set_extension("anim");
                }
                write_animation_atomic(&path, &prepared.bytes).map_err(|error| {
                    log::warn!("could not save recorded webcam animation: {error}");
                    Failure::Write
                })?;
                Ok(Output::Saved)
            });
            let _ = send.send(result);
            ctx.request_repaint();
        });
        if spawned.is_ok() {
            self.job = Some(receive);
        } else {
            self.failure = Some(Failure::Write);
        }
    }

    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        let result = match job.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(Failure::Write),
        };
        self.job = None;
        match result {
            Ok(Output::Saved) => self.saved = true,
            Ok(Output::Upload(upload)) => self.upload = Some(upload),
            Ok(Output::Cancelled) => {}
            Err(failure) => self.failure = Some(failure),
        }
    }

    /// Returns true when the take has taken over the local avatar preview.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, palette: &Palette, can_record: bool) -> bool {
        self.poll();
        let now = Instant::now();
        let was_visible = self.visible;
        widgets::section_header(ui, palette, "video-filled", &tr!("webcam-record-title"));
        ui.horizontal_wrapped(|ui| {
            if let Some(active) = &self.active {
                let status = if now < active.starts {
                    tr!(
                        "webcam-record-counting",
                        seconds = format!("{:.0}", active.starts.duration_since(now).as_secs_f32().ceil())
                    )
                } else {
                    tr!(
                        "webcam-record-running",
                        seconds = format!("{:.1}", now.duration_since(active.starts).as_secs_f32().min(active.limit))
                    )
                };
                let stop = ui.button(tr!("webcam-record-stop")).clicked();
                ui.colored_label(palette.danger, status);
                ui.ctx().request_repaint_after(Duration::from_millis(33));
                if stop {
                    self.finish();
                }
            } else {
                if ui
                    .add_enabled(can_record && self.job.is_none(), egui::Button::new(tr!("webcam-record-start")))
                    .clicked()
                {
                    self.start(now);
                }
                widgets::checkbox(ui, palette, &mut self.countdown, tr!("webcam-record-countdown"));
                ui.label(tr!("webcam-record-limit"));
                ui.add(egui::DragValue::new(&mut self.limit).range(1..=60).suffix(" s"));
            }
        });
        if self.active.is_none()
            && let Some(clip) = &self.clip
        {
            let duration = clip.duration;
            ui.horizontal_wrapped(|ui| {
                ui.label(tr!("webcam-record-in"));
                let before = ui
                    .add(
                        egui::DragValue::new(&mut self.range[0])
                            .range(0.0..=(duration - INTERVAL).max(0.0))
                            .speed(0.03)
                            .fixed_decimals(2),
                    )
                    .changed();
                if before {
                    self.range[1] = self.range[1].max(self.range[0] + INTERVAL).min(duration);
                }
                ui.label(tr!("webcam-record-out"));
                let earliest_end = (self.range[0] + INTERVAL).min(duration);
                let after = ui
                    .add(
                        egui::DragValue::new(&mut self.range[1])
                            .range(earliest_end..=duration)
                            .speed(0.03)
                            .fixed_decimals(2),
                    )
                    .changed();
                if before || after {
                    self.playing = None;
                    self.cursor = self.cursor.clamp(self.range[0], self.range[1]);
                }
            });
            if ui
                .add(
                    egui::Slider::new(&mut self.cursor, self.range[0]..=self.range[1])
                        .suffix(" s")
                        .fixed_decimals(2),
                )
                .changed()
            {
                self.visible = true;
                self.playing = None;
            }
            ui.horizontal_wrapped(|ui| {
                if self.playing.is_some() {
                    if ui.button(tr!("webcam-record-pause")).clicked() {
                        self.playing = None;
                    }
                    ui.ctx().request_repaint();
                } else if ui.button(tr!("webcam-record-preview")).clicked() {
                    if self.cursor >= self.range[1] {
                        self.cursor = self.range[0];
                    }
                    self.visible = true;
                    self.playing = Some((now, self.cursor));
                }
                if self.visible && ui.button(tr!("webcam-record-close-preview")).clicked() {
                    self.hide_preview();
                }
                widgets::checkbox(ui, palette, &mut self.looping, tr!("webcam-record-loop"));
            });
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(self.job.is_none(), |ui| {
                    if ui.button(tr!("webcam-record-save")).clicked() {
                        self.export(false, ui.ctx());
                    }
                    if ui.button(tr!("webcam-record-upload")).clicked() {
                        self.hide_preview();
                        self.export(true, ui.ctx());
                    }
                });
            });
        }
        if self.saved {
            ui.colored_label(palette.success, tr!("webcam-record-saved"));
        }
        if let Some(failure) = self.failure {
            ui.colored_label(
                palette.danger,
                match failure {
                    Failure::Empty => tr!("webcam-record-empty"),
                    Failure::Size => tr!("webcam-record-size"),
                    Failure::Write => tr!("webcam-record-error"),
                },
            );
        }
        !was_visible && self.visible
    }
}

fn assemble(samples: Vec<Sample>, names: Vec<String>) -> Option<Animation> {
    let duration = samples.last()?.time;
    if samples.len() < 2 || duration < INTERVAL {
        return None;
    }
    let mut tracks = vec![Vec::<RotKey>::new(); names.len()];
    for sample in samples {
        for (track, rotation) in tracks.iter_mut().zip(sample.rotations) {
            let Some(rotation) = rotation.filter(|q| q.is_finite()) else {
                continue;
            };
            if let Some(last) = track.last().filter(|last| sample.time - last.time > 0.24) {
                track.push(RotKey {
                    time: sample.time - 0.11,
                    rotation: last.rotation,
                });
            }
            track.push(RotKey {
                time: sample.time,
                rotation,
            });
        }
    }
    let joints: Vec<_> = names
        .into_iter()
        .zip(tracks)
        .filter_map(|(joint_name, rot_keys)| {
            (joint_name.starts_with('m') && !rot_keys.is_empty()).then_some(JointMotion {
                joint_name,
                priority: -1,
                rot_keys,
                pos_keys: Vec::new(),
            })
        })
        .collect();
    (!joints.is_empty()).then_some(Animation {
        base_priority: 4,
        duration,
        emote_name: String::new(),
        loop_in: 0.0,
        loop_out: duration,
        looping: false,
        ends_after: None,
        ease_in: 0.10,
        ease_out: 0.15,
        hand_pose: 0,
        joints,
    })
}

fn simplify(keys: &[RotKey], degrees: f32) -> Vec<RotKey> {
    let mut keep = vec![false; keys.len()];
    let last = keys.len() - 1;
    keep[0] = true;
    keep[last] = true;
    let min_dot = (degrees.to_radians() * 0.5).cos();
    let mut segments = vec![(0, last)];
    while let Some((a, b)) = segments.pop() {
        let span = keys[b].time - keys[a].time;
        let mut split = None;
        let mut worst = min_dot;
        for i in a + 1..b {
            let u = (keys[i].time - keys[a].time) / span;
            let q = keys[a].rotation.slerp(keys[b].rotation, u);
            let similarity = q.dot(keys[i].rotation).abs();
            if similarity < worst {
                worst = similarity;
                split = Some(i);
            }
        }
        if let Some(i) = split {
            keep[i] = true;
            segments.extend([(a, i), (i, b)]);
        }
    }
    keys.iter().zip(keep).filter_map(|(key, keep)| keep.then(|| key.clone())).collect()
}

fn prepare(source: &Animation, [start, end]: [f32; 2], looping: bool, name: String) -> Result<PreparedAnimation, Failure> {
    let mut clip = source.clone();
    clip.duration = end - start;
    clip.loop_out = clip.duration;
    clip.looping = looping;
    for (track, src) in clip.joints.iter_mut().zip(&source.joints) {
        let first = src.rotation_at(start).ok_or(Failure::Empty)?;
        let last = src.rotation_at(end).ok_or(Failure::Empty)?;
        track.rot_keys.clear();
        track.rot_keys.push(RotKey {
            time: 0.0,
            rotation: first,
        });
        track
            .rot_keys
            .extend(src.rot_keys.iter().filter(|k| k.time > start && k.time < end).map(|k| RotKey {
                time: k.time - start,
                rotation: k.rotation,
            }));
        track.rot_keys.push(RotKey {
            time: clip.duration,
            rotation: last,
        });
    }
    let tracks = clip.joints.clone();
    for degrees in [0.75, 1.25, 2.0] {
        for (joint, raw) in clip.joints.iter_mut().zip(&tracks) {
            joint.rot_keys = simplify(&raw.rot_keys, degrees);
        }
        let bytes = aurora_assets::import_animation::encode(&clip).map_err(|_| Failure::Write)?;
        if bytes.len() <= ASSET_BYTES {
            let animation = aurora_assets::anim::parse_animation_upload(&bytes).map_err(|_| Failure::Write)?;
            return Ok(PreparedAnimation {
                name,
                bytes: Arc::new(bytes),
                animation: Arc::new(animation),
            });
        }
    }
    Err(Failure::Size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_native_take_preserves_authored_bones_and_time() {
        let times = [0.0, 0.1, 0.2, 0.3, 0.4];
        let source = assemble(
            times
                .into_iter()
                .map(|time| Sample {
                    time,
                    rotations: vec![Some(Quat::from_rotation_y(time * 2.0)), None, Some(Quat::IDENTITY)],
                })
                .collect(),
            vec!["mChest".into(), "mWristLeft".into(), "PELVIS".into()],
        )
        .unwrap();
        let out = prepare(&source, [0.1, 0.35], true, "Take".into()).unwrap();
        let clip = &out.animation;
        assert!((clip.duration - 0.25).abs() < 1.0e-6);
        assert_eq!(clip.base_priority, 4);
        assert!(clip.looping);
        assert_eq!(clip.joints.len(), 1);
        let track = &clip.joints[0];
        assert_eq!(track.joint_name, "mChest");
        assert_eq!(track.rot_keys.len(), 2);
        assert!(track.pos_keys.is_empty());
        let actual = track.rotation_at(0.125).unwrap();
        assert!(actual.dot(Quat::from_rotation_y(0.45)).abs() > 0.99999);
    }

    #[test]
    fn simplification_keeps_a_fast_gesture_between_matching_endpoints() {
        let keys = [0.0, 0.75, 0.0]
            .into_iter()
            .enumerate()
            .map(|(i, r)| RotKey {
                time: i as f32 * INTERVAL,
                rotation: Quat::from_rotation_z(r),
            })
            .collect::<Vec<_>>();
        assert_eq!(simplify(&keys, 0.75), keys);
    }
}
