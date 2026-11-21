use std::{path::PathBuf, sync::mpsc, time::Duration};

use aurora_i18n::tr;

use super::{Camera, Channels, Device, PreviewImage, Source, WINDOW, Webcam, companion};
use crate::{theme::Palette, ui::widgets};

pub(super) struct Panel {
    pub source: Source,
    pub loop_video: bool,
    pub channels: Channels,
    show_preview: bool,
    show_dots: bool,
    initialized: bool,
    devices: Vec<Device>,
    discovery: Option<mpsc::Receiver<Vec<Device>>>,
    picker: Option<mpsc::Receiver<Option<PathBuf>>>,
    demo_clips: Vec<PathBuf>,
    texture: Option<egui::TextureHandle>,
    groups: Vec<Vec<[f32; 2]>>,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            source: Source::default(),
            loop_video: true,
            channels: Channels::default(),
            show_preview: true,
            show_dots: true,
            initialized: false,
            devices: Vec::new(),
            discovery: None,
            picker: None,
            demo_clips: Vec::new(),
            texture: None,
            groups: Vec::new(),
        }
    }
}

impl Panel {
    fn initialize(&mut self, demo: bool) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        if demo {
            if let Some(folder) = std::env::var_os("AURORA_WEBCAM_DEMOS") {
                self.demo_clips = ["01-Face-and-blinks.mp4", "02-Hand-gestures.mp4", "03-Full-body.mp4"]
                    .map(|file| PathBuf::from(&folder).join(file))
                    .into_iter()
                    .filter(|p| p.is_file())
                    .collect();
            }
            if let Some(video) = std::env::var_os("AURORA_DEMO_WEBCAM_VIDEO") {
                self.source = Source::Video(video.into());
            }
        }
        self.refresh_devices();
    }

    fn refresh_devices(&mut self) {
        let (send, receive) = mpsc::channel();
        if std::thread::Builder::new()
            .name("webcam-devices".into())
            .spawn(move || {
                let devices = companion()
                    .arg("--list-cameras")
                    .output()
                    .ok()
                    .filter(|out| out.status.success())
                    .and_then(|out| serde_json::from_slice(&out.stdout).ok())
                    .unwrap_or_default();
                let _ = send.send(devices);
            })
            .is_ok()
        {
            self.discovery = Some(receive);
        }
    }

    fn pick_video(&mut self) {
        let (send, receive) = mpsc::channel();
        let title = tr!("webcam-browse");
        if std::thread::Builder::new()
            .name("webcam-video-picker".into())
            .spawn(move || {
                let path = rfd::FileDialog::new()
                    .set_title(title)
                    .add_filter("Video", &["mp4", "m4v", "mov", "webm", "mkv", "avi"])
                    .pick_file();
                let _ = send.send(path);
            })
            .is_ok()
        {
            self.picker = Some(receive);
        }
    }

    fn poll(&mut self) -> Option<PathBuf> {
        if let Some(discovery) = &self.discovery {
            match discovery.try_recv() {
                Ok(devices) => {
                    self.devices = devices;
                    self.discovery = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.discovery = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let picked = self.picker.as_ref()?.try_recv();
        if matches!(picked, Err(mpsc::TryRecvError::Empty)) {
            return None;
        }
        self.picker = None;
        picked.ok().flatten()
    }

    pub fn clear_preview(&mut self) {
        self.texture = None;
        self.groups.clear();
    }

    fn update_preview(&mut self, ctx: &egui::Context, preview: PreviewImage) {
        self.groups = preview.groups;
        if let Some(texture) = &mut self.texture {
            texture.set(preview.image, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture(WINDOW, preview.image, egui::TextureOptions::LINEAR));
        }
    }

    fn chooser(&mut self, ui: &mut egui::Ui) {
        let source = &mut self.source;
        let name = match source {
            Source::Camera(camera) if *camera == Camera::default() && !self.devices.is_empty() => tr!("webcam-camera-auto").to_owned(),
            Source::Camera(camera) => self
                .devices
                .iter()
                .find(|device| device.camera == *camera)
                .map_or_else(|| tr!("webcam-camera-number", number = camera.index), |device| device.name.clone()),
            Source::Video(file) => file_label(file),
        };
        egui::ComboBox::from_id_salt("webcam-source")
            .width(255.0)
            .selected_text(name)
            .show_ui(ui, |ui| {
                for device in &self.devices {
                    ui.selectable_value(source, Source::Camera(device.camera), &device.name);
                }
                if self.devices.is_empty() {
                    for index in 0..6 {
                        ui.selectable_value(
                            source,
                            Source::Camera(Camera { index, backend: 0 }),
                            tr!("webcam-camera-number", number = index),
                        );
                    }
                } else {
                    ui.selectable_value(source, Source::default(), tr!("webcam-camera-auto"));
                }
                for file in &self.demo_clips {
                    ui.selectable_value(source, Source::Video(file.clone()), file_label(file));
                }
            });
    }

    fn next_camera(&mut self) {
        let index = match self.source {
            Source::Camera(camera) => self.devices.iter().position(|device| device.camera == camera),
            Source::Video(_) => None,
        };
        let next = index.map_or(0, |index| (index + 1) % self.devices.len());
        self.source = Source::Camera(self.devices[next].camera);
    }

    fn preview(&self, ui: &mut egui::Ui, palette: &Palette, mirror: bool) {
        let Some(texture) = self.texture.as_ref().filter(|_| self.show_preview) else {
            return;
        };
        let size = texture.size_vec2();
        let fit = (ui.available_width() / size.x).min(180.0 / size.y);
        let uv = if mirror {
            egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0))
        } else {
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0))
        };
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - size.x * fit).max(0.0) * 0.5);
            let image = ui.add(egui::Image::new((texture.id(), size * fit)).uv(uv));
            if self.show_dots {
                let painter = ui.painter().with_clip_rect(image.rect);
                for (points, color) in self
                    .groups
                    .iter()
                    .zip([palette.amber, palette.violet_light, palette.teal, palette.rose])
                {
                    for &[x, y] in points {
                        let x = if mirror { 1.0 - x } else { x };
                        painter.circle_filled(image.rect.lerp_inside(egui::vec2(x, y)), 1.5, color);
                    }
                }
            }
        });
    }
}

fn file_label(file: &std::path::Path) -> String {
    file.file_name().unwrap_or(file.as_os_str()).to_string_lossy().into_owned()
}

impl Webcam {
    pub fn show(&mut self, ctx: &egui::Context, palette: &Palette, demo: bool) {
        if !self.open {
            return;
        }
        self.panel.initialize(demo);
        let was_running = self.capture.is_some();
        let previous_source = self.panel.source.clone();
        let previous_loop = self.panel.loop_video;
        if let Some(video) = self.panel.poll() {
            self.panel.source = Source::Video(video);
        }
        if let Some(preview) = self.preview.take() {
            self.panel.update_preview(ctx, preview);
        }
        let mut open = self.open;
        let mut floater = widgets::Floater::new(WINDOW, tr!("webcam-title"), egui::pos2(72.0, 120.0), egui::vec2(420.0, 525.0))
            .framed_header("video-filled");
        floater.min_size = egui::vec2(380.0, 440.0);
        floater.show(ctx, palette, &mut open, |ui| {
            self.source_controls(ui, palette);
            self.capture_controls(ui, palette);
            widgets::divider(ui, palette);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.controls(ui, palette));
        });
        self.open = open;
        if !open {
            self.stop();
        } else if was_running
            && self.capture.is_some()
            && (self.panel.source != previous_source
                || (matches!(self.panel.source, Source::Video(_)) && self.panel.loop_video != previous_loop))
        {
            self.start();
        }
    }

    fn source_controls(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        widgets::section_header(ui, palette, "video-filled", &tr!("webcam-input"));
        ui.horizontal_wrapped(|ui| {
            self.panel.chooser(ui);
            if ui
                .add_enabled(self.panel.discovery.is_none(), egui::Button::new(tr!("webcam-refresh")))
                .clicked()
            {
                self.panel.refresh_devices();
            }
            if self.panel.devices.len() > 1 && ui.button(tr!("webcam-next-camera")).clicked() {
                self.panel.next_camera();
            }
            if ui
                .add_enabled(self.panel.picker.is_none(), egui::Button::new(tr!("webcam-browse")))
                .clicked()
            {
                self.panel.pick_video();
            }
        });
        if matches!(self.panel.source, Source::Video(_)) {
            widgets::checkbox(ui, palette, &mut self.panel.loop_video, tr!("webcam-loop"));
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        self.panel.preview(ui, palette, self.mirror);
        ui.horizontal_wrapped(|ui| {
            widgets::checkbox(ui, palette, &mut self.panel.show_preview, tr!("webcam-preview"));
            ui.add_enabled_ui(self.panel.show_preview, |ui| {
                widgets::checkbox(ui, palette, &mut self.panel.show_dots, tr!("webcam-landmarks"));
            });
            widgets::checkbox(ui, palette, &mut self.mirror, tr!("webcam-mirror"));
            self.front_view_requested |= ui.button(tr!("camera-settings-front-view")).clicked();
        });

        widgets::divider(ui, palette);
        widgets::section_header(ui, palette, "bone-filled", &tr!("webcam-channels"));
        let ch = &mut self.panel.channels;
        ui.horizontal_wrapped(|ui| {
            widgets::checkbox(ui, palette, &mut ch.head, tr!("webcam-head"));
            widgets::checkbox(ui, palette, &mut ch.eyes, tr!("webcam-eyes"));
            widgets::checkbox(ui, palette, &mut ch.expression, tr!("webcam-expression"));
            widgets::checkbox(ui, palette, &mut ch.hands, tr!("webcam-hands"));
            widgets::checkbox(ui, palette, &mut ch.upper_body, tr!("webcam-upper-body"));
        });
        ui.add_enabled_ui(ch.expression, |ui| {
            ui.add(egui::Slider::new(&mut ch.expression_strength, 0.0..=2.0).text(tr!("webcam-expression-strength")));
        });
        ui.colored_label(palette.muted, tr!("webcam-local"));
    }

    fn capture_controls(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        ui.horizontal_wrapped(|ui| {
            if self.capture.is_some() {
                let fresh = self.latest.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_millis(500));
                if fresh.is_some_and(|(_, frame)| frame.detected()) {
                    ui.colored_label(palette.success, tr!("webcam-fps", fps = format!("{:.0}", self.fps)));
                } else {
                    ui.label(tr!("webcam-waiting"));
                }
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
                let label = if matches!(self.panel.source, Source::Video(_)) {
                    tr!("webcam-replay")
                } else {
                    tr!("webcam-start")
                };
                if widgets::primary_button_sized(ui, palette, label, egui::vec2(124.0, 30.0)).clicked() {
                    self.start();
                }
            }
        });
        if self.error {
            ui.colored_label(palette.danger, tr!("webcam-error"));
        }
    }
}
