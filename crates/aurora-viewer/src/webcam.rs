//! Webcam landmarks arrive over a private child-process pipe. Only the
//! viewer's own rendered skeleton consumes them.

mod retarget;

use std::{
    ffi::OsStr,
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{theme::Palette, ui::widgets};
use aurora_i18n::tr;
use serde::Deserialize;

pub(crate) use retarget::Pose;

pub const WINDOW: &str = "webcam_tracking";
const SCRIPT: &str = include_str!("webcam_capture.py");
const FRAME_BYTES: u64 = 16_384;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Frame {
    body: [Option<[f32; 3]>; 8],
    left: Option<[[f32; 3]; 21]>,
    right: Option<[[f32; 3]; 21]>,
    face: Option<[[f32; 3]; 16]>,
}

type Latest = Arc<Mutex<Option<(Instant, Frame)>>>;

struct Capture {
    child: Child,
    latest: Latest,
}

impl Capture {
    fn start(camera: u8, video: Option<&OsStr>) -> std::io::Result<Self> {
        let python = std::env::var_os("AURORA_WEBCAM_PYTHON").unwrap_or_else(|| "python".into());
        let mut command = Command::new(python);
        command
            .args(["-I", "-u", "-c", SCRIPT, "--camera", &camera.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        if let Some(video) = video {
            command.arg("--video").arg(video);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
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
                if let Ok(frame) = serde_json::from_slice::<Frame>(&line)
                    && let Ok(mut slot) = latest.lock()
                {
                    *slot = Some((Instant::now(), frame));
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
    camera: u8,
    mirror: bool,
    error: bool,
    latest: Option<(Instant, Frame)>,
    neutral: Option<Frame>,
}

impl Webcam {
    pub fn stop(&mut self) {
        self.capture = None;
        self.latest = None;
        self.neutral = None;
    }

    pub fn update(&mut self) -> Option<Arc<Pose>> {
        let capture = self.capture.as_mut()?;
        let exit = capture.child.try_wait();
        if !matches!(exit, Ok(None)) {
            self.stop();
            self.error = !matches!(exit, Ok(Some(status)) if status.success());
            return None;
        }
        if let Ok(mut pending) = capture.latest.try_lock()
            && let Some((when, frame)) = pending.take()
        {
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
        let pose = if self.mirror {
            Pose::new(&frame.mirrored(), self.neutral.as_ref().map(Frame::mirrored).as_ref(), strength)
        } else {
            Pose::new(frame, self.neutral.as_ref(), strength)
        };
        Some(Arc::new(pose))
    }

    pub fn show(&mut self, ctx: &egui::Context, palette: &Palette, demo: bool) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let video = demo.then(|| std::env::var_os("AURORA_DEMO_WEBCAM_VIDEO")).flatten();
        widgets::Floater::new(WINDOW, tr!("webcam-title"), egui::pos2(72.0, 120.0), egui::vec2(350.0, 220.0))
            .framed_header("video-filled")
            .show(ctx, palette, &mut open, |ui| {
                ui.label(tr!("webcam-local"));
                if video.is_none() {
                    ui.horizontal(|ui| {
                        ui.label(tr!("webcam-camera-index"));
                        ui.add_enabled(self.capture.is_none(), egui::DragValue::new(&mut self.camera).range(0..=15));
                    });
                }
                widgets::checkbox(ui, palette, &mut self.mirror, tr!("webcam-mirror"));
                if self.capture.is_some() {
                    let fresh = self.latest.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_millis(500));
                    let tracked = fresh.is_some_and(|(_, frame)| frame.detected());
                    ui.label(if tracked { tr!("webcam-active") } else { tr!("webcam-waiting") });
                    if ui
                        .add_enabled(
                            fresh.is_some_and(|(_, f)| f.face.is_some()),
                            egui::Button::new(tr!("webcam-recenter")),
                        )
                        .clicked()
                    {
                        self.neutral = fresh.map(|(_, frame)| frame.clone());
                    }
                    if ui.button(tr!("webcam-stop")).clicked() {
                        self.stop();
                    }
                } else {
                    if self.error {
                        ui.label(tr!("webcam-error"));
                    }
                    let start = if video.is_some() {
                        tr!("webcam-replay")
                    } else {
                        tr!("webcam-start")
                    };
                    if ui.button(start).clicked() {
                        self.capture = match Capture::start(self.camera, video.as_deref()) {
                            Ok(capture) => Some(capture),
                            Err(error) => {
                                log::warn!("cannot launch webcam tracking: {error}");
                                None
                            }
                        };
                        self.error = self.capture.is_none();
                    }
                }
            });
        self.open = open;
        if !open {
            self.stop();
        }
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
