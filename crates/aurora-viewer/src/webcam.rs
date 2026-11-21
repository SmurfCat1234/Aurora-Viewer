//! Webcam landmarks arrive over a private child-process pipe. Only the
//! viewer's own rendered skeleton consumes them.

mod panel;
mod retarget;

use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    time::Instant,
};

use base64::Engine;
use serde::Deserialize;

use retarget::Channels;
pub(crate) use retarget::Pose;

pub const WINDOW: &str = "webcam_tracking";
const SCRIPT: &str = include_str!("webcam_capture.py");
const FRAME_BYTES: u64 = 131_072;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Frame {
    body: [Option<[f32; 3]>; 8],
    left: Option<[[f32; 3]; 21]>,
    right: Option<[[f32; 3]; 21]>,
    face: Option<[[f32; 3]; 16]>,
}

#[derive(Deserialize)]
struct Sample {
    id: Option<u64>,
    frame: Option<Frame>,
    preview: Option<PreviewFrame>,
    ended: Option<bool>,
    error: Option<String>,
    fatal: Option<String>,
}

#[derive(Deserialize)]
struct PreviewFrame {
    jpeg: String,
    groups: Vec<Vec<[f32; 2]>>,
}

struct PreviewImage {
    image: egui::ColorImage,
    groups: Vec<Vec<[f32; 2]>>,
}

impl PreviewFrame {
    fn decode(self) -> Option<PreviewImage> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(self.jpeg).ok()?;
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(384);
        limits.max_image_height = Some(384);
        reader.limits(limits);
        let rgb = reader.decode().ok()?.to_rgb8();
        let size = [rgb.width() as usize, rgb.height() as usize];
        Some(PreviewImage {
            image: egui::ColorImage::from_rgb(size, rgb.as_raw()),
            groups: self.groups,
        })
    }
}

#[derive(Default)]
struct Latest {
    id: Option<u64>,
    frame: Option<(Instant, Frame)>,
    preview: Option<PreviewImage>,
    finished: Option<bool>,
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
struct Camera {
    index: i32,
    backend: i32,
}

#[derive(Deserialize)]
struct Device {
    #[serde(flatten)]
    camera: Camera,
    name: String,
}

#[derive(Clone, PartialEq, Eq)]
enum Source {
    Camera(Camera),
    Video(PathBuf),
}

impl Default for Source {
    fn default() -> Self {
        Self::Camera(Camera::default())
    }
}

struct Capture {
    child: Child,
    input: ChildStdin,
    serial: u64,
    latest: Arc<Mutex<Latest>>,
}

fn companion() -> Command {
    let python = std::env::var_os("AURORA_WEBCAM_PYTHON").unwrap_or_else(|| "python".into());
    let mut command = Command::new(python);
    command.args(["-I", "-u", "-c", SCRIPT]).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

impl Capture {
    fn launch() -> std::io::Result<Self> {
        let mut command = companion();
        command.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = command.spawn()?;
        let input = child.stdin.take().expect("piped webcam stdin");
        let stdout = child.stdout.take().expect("piped webcam stdout");
        let capture = Self {
            child,
            input,
            serial: 0,
            latest: Arc::default(),
        };
        let latest = capture.latest.clone();
        std::thread::Builder::new().name("webcam-landmarks".into()).spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.by_ref().take(FRAME_BYTES).read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if line.last() != Some(&b'\n') => break,
                    Ok(_) => {}
                }
                if let Ok(sample) = serde_json::from_slice::<Sample>(&line) {
                    let preview = sample.preview.and_then(PreviewFrame::decode);
                    if let Ok(mut slot) = latest.lock() {
                        if let Some(error) = sample.fatal {
                            log::warn!("webcam models unavailable: {error}");
                            slot.finished = Some(true);
                            continue;
                        }
                        if sample.id.is_none() || slot.id != sample.id {
                            continue;
                        }
                        if let Some(error) = sample.error {
                            log::warn!("webcam input failed: {error}");
                            slot.finished = Some(true);
                        } else if sample.ended == Some(true) {
                            slot.finished = Some(false);
                        }
                        if let Some(frame) = sample.frame {
                            slot.frame = Some((Instant::now(), frame));
                        }
                        if preview.is_some() {
                            slot.preview = preview;
                        }
                    }
                }
            }
        })?;
        Ok(capture)
    }

    fn send(&mut self, message: serde_json::Value) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(&message)?;
        line.push(b'\n');
        self.input.write_all(&line)
    }

    fn start(&mut self, source: &Source, loop_video: bool) -> std::io::Result<()> {
        self.serial += 1;
        let id = self.serial;
        *self.latest.lock().unwrap() = Latest {
            id: Some(id),
            ..Latest::default()
        };
        let message = match source {
            Source::Camera(camera) => serde_json::json!({"id": id, "camera": camera.index, "backend": camera.backend}),
            Source::Video(path) => serde_json::json!({"id": id, "video": path.to_string_lossy(), "loop_video": loop_video}),
        };
        self.send(message)
    }

    fn stop(&mut self) {
        *self.latest.lock().unwrap() = Latest::default();
        let _ = self.send(serde_json::json!({"stop": true}));
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub(crate) struct Webcam {
    pub open: bool,
    capture: Option<Capture>,
    running: bool,
    warm_attempted: bool,
    warming_before_open: bool,
    panel: panel::Panel,
    mirror: bool,
    error: bool,
    latest: Option<(Instant, Frame)>,
    rendered: Option<(Instant, Frame)>,
    neutral: Option<Frame>,
    preview: Option<PreviewImage>,
    fps: f32,
    front_view_requested: bool,
}

impl Webcam {
    pub fn take_front_view_request(&mut self) -> bool {
        std::mem::take(&mut self.front_view_requested)
    }

    pub fn stop(&mut self) {
        if self.running
            && let Some(capture) = &mut self.capture
        {
            capture.stop();
        }
        self.clear_input();
    }

    fn clear_input(&mut self) {
        self.running = false;
        self.latest = None;
        self.rendered = None;
        self.neutral = None;
        self.preview = None;
        self.panel.clear_preview();
        self.fps = 0.0;
    }

    fn warm(&mut self) -> std::io::Result<&mut Capture> {
        if let Some(capture) = &mut self.capture
            && !matches!(capture.child.try_wait(), Ok(None))
        {
            self.capture = None;
        }
        if self.capture.is_none() {
            self.capture = Some(Capture::launch()?);
        }
        Ok(self.capture.as_mut().unwrap())
    }

    fn prewarm(&mut self) {
        if !self.warm_attempted {
            self.warm_attempted = true;
            if let Err(error) = self.warm() {
                log::warn!("cannot prewarm webcam tracking: {error}");
            }
        }
    }

    pub fn prewarm_demo(&mut self) {
        self.prewarm();
        self.warming_before_open = self.capture.is_some();
    }

    fn start(&mut self) {
        self.clear_input();
        let source = self.panel.source.clone();
        let loop_video = self.panel.loop_video;
        match self.warm().and_then(|capture| capture.start(&source, loop_video)) {
            Ok(()) => {
                self.running = true;
                self.error = false;
            }
            Err(error) => {
                log::warn!("cannot launch webcam tracking: {error}");
                self.capture = None;
                self.error = true;
            }
        }
    }

    pub fn update(&mut self) -> Option<Arc<Pose>> {
        if !self.open {
            if !self.warming_before_open {
                if self.capture.take().is_some() {
                    self.clear_input();
                }
                self.warm_attempted = false;
            }
            return None;
        }
        let capture = self.capture.as_mut()?;
        let exit = capture.child.try_wait();
        if !matches!(exit, Ok(None)) {
            self.capture = None;
            self.clear_input();
            self.error = !matches!(exit, Ok(Some(status)) if status.success());
            return None;
        }
        if !self.running {
            return None;
        }
        let pending = capture
            .latest
            .try_lock()
            .ok()
            .map(|mut slot| (slot.finished.take(), slot.preview.take(), slot.frame.take()));
        if let Some((finished, preview, sample)) = pending {
            if let Some(error) = finished {
                self.stop();
                self.error = error;
                return None;
            }
            if preview.is_some() {
                self.preview = preview;
            }
            if let Some((when, frame)) = sample {
                self.update_target(when, frame);
            }
        }
        let (received, target) = self.latest.as_ref()?;
        let now = Instant::now();
        let age = now.saturating_duration_since(*received).as_secs_f32();
        let fade_after = (2.0 / self.fps.max(3.0)).clamp(0.18, 0.45);
        let strength = (1.0 - (age - fade_after) / 0.35).clamp(0.0, 1.0);
        if strength == 0.0 || !target.detected() {
            self.rendered = None;
            return None;
        }
        // Apply new landmarks across render frames so sparse estimates do not
        // move the avatar in single-frame jumps.
        let mut frame = target.clone();
        if let Some((at, old)) = &self.rendered {
            let elapsed = now.saturating_duration_since(*at).as_secs_f32();
            let time_constant = (0.7 / self.fps.max(3.0)).clamp(0.028, 0.09);
            frame.smooth_from(old, 1.0 - (-elapsed / time_constant).exp());
        }
        self.rendered = Some((now, frame.clone()));
        let channels = self.panel.channels;
        let pose = if self.mirror {
            Pose::new(
                &frame.mirrored(),
                self.neutral.as_ref().map(Frame::mirrored).as_ref(),
                strength,
                channels,
            )
        } else {
            Pose::new(&frame, self.neutral.as_ref(), strength, channels)
        };
        Some(Arc::new(pose))
    }

    fn update_target(&mut self, when: Instant, frame: Frame) {
        let neutral = self.neutral.get_or_insert_with(|| frame.clone());
        if neutral.face.is_none() {
            neutral.face = frame.face;
        }
        if neutral.body[0].is_none() || neutral.body[3].is_none() {
            neutral.body = frame.body;
        }
        if let Some((previous_at, _)) = &self.latest {
            let elapsed = when.saturating_duration_since(*previous_at).as_secs_f32();
            if elapsed > 0.001 {
                let rate = (1.0 / elapsed).min(120.0);
                self.fps = if self.fps == 0.0 { rate } else { self.fps * 0.88 + rate * 0.12 };
            }
        }
        self.latest = Some((when, frame));
    }
}

impl Frame {
    fn detected(&self) -> bool {
        self.face.is_some() || self.left.is_some() || self.right.is_some() || self.body.iter().any(Option::is_some)
    }

    fn mirrored(&self) -> Self {
        let flip = |[x, y, z]: [f32; 3]| [x, -y, z];
        let body = [3, 4, 5, 0, 1, 2, 7, 6].map(|i| self.body[i].map(flip));
        let face = self
            .face
            .map(|p| [1, 0, 2, 3, 4, 5, 7, 6, 12, 13, 14, 15, 8, 9, 10, 11].map(|i| flip(p[i])));
        Self {
            body,
            face,
            left: self.right.map(|p| p.map(flip)),
            right: self.left.map(|p| p.map(flip)),
        }
    }

    fn smooth_from(&mut self, previous: &Self, amount: f32) {
        fn smooth(point: &mut [f32; 3], previous: &[f32; 3], amount: f32) {
            for (current, old) in point.iter_mut().zip(previous) {
                *current = old + (*current - old) * amount;
            }
        }

        fn smooth_group<const N: usize>(group: &mut Option<[[f32; 3]; N]>, previous: &Option<[[f32; 3]; N]>, amount: f32) {
            if let Some((current, old)) = group.as_mut().zip(previous.as_ref()) {
                for (point, old) in current.iter_mut().zip(old) {
                    smooth(point, old, amount);
                }
            }
        }

        for (point, old) in self.body.iter_mut().zip(&previous.body) {
            if let Some((point, old)) = point.as_mut().zip(old.as_ref()) {
                smooth(point, old, amount);
            }
        }
        smooth_group(&mut self.left, &previous.left, amount);
        smooth_group(&mut self.right, &previous.right, amount);
        smooth_group(&mut self.face, &previous.face, amount);
    }
}
