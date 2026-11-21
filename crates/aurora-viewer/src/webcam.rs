//! Webcam landmarks arrive over a private child-process pipe. Only the
//! viewer's own rendered skeleton consumes them.

mod panel;
mod retarget;

use std::{
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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
    frame: Frame,
    preview: Option<PreviewFrame>,
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
    frame: Option<(Instant, Frame)>,
    preview: Option<PreviewImage>,
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
    fn start(source: &Source, loop_video: bool) -> std::io::Result<Self> {
        let mut command = companion();
        command.arg("--preview").stdout(Stdio::piped());
        match source {
            Source::Camera(camera) => {
                command.args(["--camera", &camera.index.to_string(), "--backend", &camera.backend.to_string()]);
            }
            Source::Video(path) => {
                command.arg("--video").arg(path);
                if loop_video {
                    command.arg("--loop");
                }
            }
        }
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("piped webcam stdout");
        let capture = Self {
            child,
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
                        slot.frame = Some((Instant::now(), sample.frame));
                        if preview.is_some() {
                            slot.preview = preview;
                        }
                    }
                }
            }
        })?;
        Ok(capture)
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
    panel: panel::Panel,
    mirror: bool,
    error: bool,
    latest: Option<(Instant, Frame)>,
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
        self.capture = None;
        self.latest = None;
        self.neutral = None;
        self.preview = None;
        self.panel.clear_preview();
        self.fps = 0.0;
    }

    fn start(&mut self) {
        self.stop();
        self.capture = match Capture::start(&self.panel.source, self.panel.loop_video) {
            Ok(capture) => Some(capture),
            Err(error) => {
                log::warn!("cannot launch webcam tracking: {error}");
                None
            }
        };
        self.error = self.capture.is_none();
    }

    pub fn update(&mut self) -> Option<Arc<Pose>> {
        let capture = self.capture.as_mut()?;
        let exit = capture.child.try_wait();
        if !matches!(exit, Ok(None)) {
            self.stop();
            self.error = !matches!(exit, Ok(Some(status)) if status.success());
            return None;
        }
        let sample = capture.latest.try_lock().ok().and_then(|mut pending| {
            if let Some(preview) = pending.preview.take() {
                self.preview = Some(preview);
            }
            pending.frame.take()
        });
        if let Some((when, frame)) = sample {
            let neutral = self.neutral.get_or_insert_with(|| frame.clone());
            if neutral.face.is_none() {
                neutral.face = frame.face;
            }
            if neutral.body[0].is_none() || neutral.body[3].is_none() {
                neutral.body = frame.body;
            }
            let mut frame = frame;
            if let Some((previous_at, previous)) = &self.latest {
                let elapsed = when.saturating_duration_since(*previous_at).as_secs_f32();
                frame.smooth_from(previous, 1.0 - (-elapsed / 0.045).exp());
                if elapsed > 0.001 {
                    let rate = (1.0 / elapsed).min(120.0);
                    self.fps = if self.fps == 0.0 { rate } else { self.fps * 0.88 + rate * 0.12 };
                }
            }
            self.latest = Some((when, frame));
        }
        let (received, frame) = self.latest.as_ref()?;
        if !frame.detected() {
            return None;
        }
        let age = received.elapsed();
        if age >= Duration::from_millis(500) {
            return None;
        }
        let strength = (1.0 - (age.as_secs_f32() - 0.15) / 0.35).clamp(0.0, 1.0);
        let channels = self.panel.channels;
        let pose = if self.mirror {
            Pose::new(
                &frame.mirrored(),
                self.neutral.as_ref().map(Frame::mirrored).as_ref(),
                strength,
                channels,
            )
        } else {
            Pose::new(frame, self.neutral.as_ref(), strength, channels)
        };
        Some(Arc::new(pose))
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
