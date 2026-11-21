use std::sync::atomic::Ordering;
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use aurora_i18n::tr;

use super::{
    Camera, Channels, Device, FileKind, IMAGE_EXTENSIONS, LinkError, PreviewImage, Source, VIDEO_EXTENSIONS, WINDOW, Webcam,
    enumerate_cameras,
};
use crate::{theme::Palette, ui::widgets};

pub(super) struct Panel {
    pub source: Source,
    pub loop_video: bool,
    pub channels: Channels,
    show_dots: bool,
    initialized: bool,
    devices: Vec<Device>,
    discovery: Option<mpsc::Receiver<Vec<Device>>>,
    picker: Option<mpsc::Receiver<Option<PathBuf>>>,
    link: String,
    demo_clips: Vec<PathBuf>,
    texture: Option<egui::TextureHandle>,
    last_preview: Option<Instant>,
    preview_fps: f32,
    groups: Vec<Vec<[f32; 2]>>,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            source: Source::default(),
            loop_video: true,
            channels: Channels::default(),
            show_dots: true,
            initialized: false,
            devices: Vec::new(),
            discovery: None,
            picker: None,
            link: String::new(),
            demo_clips: Vec::new(),
            texture: None,
            last_preview: None,
            preview_fps: 0.0,
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
            if let Some(folder) = std::env::var_os("AURORA_WEBCAM_DEMOS")
                && let Ok(files) = std::fs::read_dir(folder)
            {
                self.demo_clips = files
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| FileKind::of(path).is_some() && path.is_file())
                    .collect();
                self.demo_clips.sort();
            }
            if let Some(video) = std::env::var_os("AURORA_DEMO_WEBCAM_VIDEO") {
                self.source = Source::file(video.into());
            }
        }
        self.refresh_devices();
    }

    fn refresh_devices(&mut self) {
        let (send, receive) = mpsc::channel();
        if std::thread::Builder::new()
            .name("webcam-devices".into())
            .spawn(move || {
                let devices = enumerate_cameras().unwrap_or_default();
                let _ = send.send(devices);
            })
            .is_ok()
        {
            self.discovery = Some(receive);
        }
    }

    fn pick_file(&mut self) {
        let (send, receive) = mpsc::channel();
        let title = tr!("webcam-browse");
        if std::thread::Builder::new()
            .name("webcam-file-picker".into())
            .spawn(move || {
                let path = rfd::FileDialog::new()
                    .set_title(title)
                    .add_filter(tr!("webcam-media-files"), &[VIDEO_EXTENSIONS, IMAGE_EXTENSIONS].concat())
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
        self.last_preview = None;
        self.preview_fps = 0.0;
        self.groups.clear();
    }

    fn update_preview(&mut self, ctx: &egui::Context, preview: PreviewImage) {
        let now = Instant::now();
        if let Some(previous) = self.last_preview.replace(now) {
            let rate = now.saturating_duration_since(previous).as_secs_f32().recip().min(120.0);
            self.preview_fps = if self.preview_fps == 0.0 {
                rate
            } else {
                self.preview_fps * 0.88 + rate * 0.12
            };
        }
        self.groups = preview.groups;
        if let Some(texture) = &mut self.texture {
            texture.set(preview.image, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture(WINDOW, preview.image, egui::TextureOptions::LINEAR));
        }
    }

    fn chooser(&mut self, ui: &mut egui::Ui) -> bool {
        let source = &mut self.source;
        let name = match source {
            Source::Camera(camera) if *camera == Camera::default() && !self.devices.is_empty() => tr!("webcam-camera-auto").to_owned(),
            Source::Camera(camera) => self
                .devices
                .iter()
                .find(|device| device.camera == *camera)
                .map_or_else(|| tr!("webcam-camera-number", number = camera.index), |device| device.name.clone()),
            Source::File(file, _) => file_label(file),
            Source::Link(_) => tr!("webcam-link-source").to_owned(),
        };
        let mut play_video = false;
        egui::ComboBox::from_id_salt("webcam-source")
            .width(ui.available_width())
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
                    play_video |= ui.selectable_value(source, Source::file(file.clone()), file_label(file)).clicked();
                }
            });
        play_video
    }

    fn next_camera(&mut self) {
        let index = match self.source {
            Source::Camera(camera) => self.devices.iter().position(|device| device.camera == camera),
            Source::File(..) | Source::Link(_) => None,
        };
        let next = index.map_or(0, |index| (index + 1) % self.devices.len());
        self.source = Source::Camera(self.devices[next].camera);
    }

    fn preview(&self, ui: &mut egui::Ui, palette: &Palette, mirror: bool, empty: &str) {
        let (stage, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 180.0), egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(stage);
        painter.rect_filled(stage, ui.visuals().widgets.inactive.corner_radius, palette.field);
        if let Some(texture) = self.texture.as_ref().filter(|_| self.last_preview.is_some()) {
            let size = texture.size_vec2();
            let bounds = stage.shrink(4.0);
            let fit = (bounds.width() / size.x).min(bounds.height() / size.y);
            let image = egui::Rect::from_center_size(bounds.center(), size * fit);
            let uv = if mirror {
                egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0))
            } else {
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0))
            };
            painter.image(texture.id(), image, uv, egui::Color32::WHITE); // theme-guard: content — the video keeps its original colors
            if self.show_dots {
                let painter = painter.with_clip_rect(image);
                let project = |[x, y]: [f32; 2]| image.lerp_inside(egui::vec2(if mirror { 1.0 - x } else { x }, y));
                if let Some(bones) = self.groups.get(4) {
                    for segment in bones.chunks_exact(2) {
                        painter.line_segment([project(segment[0]), project(segment[1])], egui::Stroke::new(1.25, palette.amber));
                    }
                }
                for (points, color) in self
                    .groups
                    .iter()
                    .zip([palette.amber, palette.violet_light, palette.teal, palette.rose])
                {
                    for &point in points {
                        painter.circle_filled(project(point), 1.5, color);
                    }
                }
            }
        } else {
            painter.text(
                stage.center(),
                egui::Align2::CENTER_CENTER,
                empty,
                egui::TextStyle::Body.resolve(ui.style()),
                palette.muted,
            );
        }
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
        self.warming_before_open = false;
        self.prewarm();
        let was_running = self.running;
        let previous_source = self.panel.source.clone();
        let previous_loop = self.panel.loop_video;
        let mut play_video = false;
        if let Some(video) = self.panel.poll() {
            self.panel.source = Source::file(video);
            play_video = true;
        }
        if let Some(preview) = self.preview.take() {
            self.panel.update_preview(ctx, preview);
        }
        let mut open = self.open;
        let mut floater = widgets::Floater::new(WINDOW, tr!("webcam-title"), egui::pos2(72.0, 120.0), egui::vec2(460.0, 585.0))
            .framed_header("video-filled");
        floater.min_size = egui::vec2(390.0, 485.0);
        floater.show(ctx, palette, &mut open, |ui| {
            play_video |= self.source_controls(ui, palette);
            ui.add_space(4.0);
            self.capture_controls(ui, palette);
            ui.add_space(6.0);
            self.preview_stage(ui, palette);
            widgets::divider(ui, palette);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| self.controls(ui, palette));
        });
        self.open = open;
        if !open {
            self.capture = None;
            self.clear_input();
            self.recorder.hide_preview();
        } else if play_video
            || (was_running
                && self.running
                && (self.panel.source != previous_source || (self.can_loop() && self.panel.loop_video != previous_loop)))
        {
            self.start();
        }
    }

    fn source_controls(&mut self, ui: &mut egui::Ui, palette: &Palette) -> bool {
        widgets::section_header(ui, palette, "video-filled", &tr!("webcam-input"));
        let mut play_video = self.panel.chooser(ui);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.panel.discovery.is_none(), egui::Button::new(tr!("webcam-refresh")))
                .clicked()
            {
                self.panel.refresh_devices();
            }
            if ui
                .add_enabled(self.panel.devices.len() > 1, egui::Button::new(tr!("webcam-next-camera")))
                .clicked()
            {
                self.panel.next_camera();
            }
            if ui
                .add_enabled(self.panel.picker.is_none(), egui::Button::new(tr!("webcam-browse")))
                .clicked()
            {
                self.panel.pick_file();
            }
        });
        ui.horizontal(|ui| {
            let label = tr!("webcam-link-play");
            let font = egui::TextStyle::Button.resolve(ui.style());
            let button_width = ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.to_owned(), font, egui::Color32::WHITE).size().x);
            let button_width = (button_width + 22.0).max(90.0);
            let field = ui.add_sized(
                [(ui.available_width() - button_width - ui.spacing().item_spacing.x).max(120.0), 26.0],
                egui::TextEdit::singleline(&mut self.panel.link)
                    .char_limit(4096)
                    .hint_text(tr!("webcam-social-placeholder")),
            );
            let url = self.panel.link.trim();
            let valid = url.split_once("://").is_some_and(|(scheme, rest)| {
                (scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")) && !rest.is_empty()
            });
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if ui.add_enabled(valid, egui::Button::new(label)).clicked() || (valid && enter) {
                self.panel.source = Source::Link(url.to_owned());
                play_video = true;
            }
        });
        play_video
    }

    fn preview_stage(&self, ui: &mut egui::Ui, palette: &Palette) {
        let fresh = self
            .latest
            .as_ref()
            .filter(|(at, _)| self.still || at.elapsed() < Duration::from_millis(500));
        ui.horizontal(|ui| {
            if !self.still && self.panel.preview_fps > 0.0 {
                ui.strong(tr!("webcam-preview-fps", fps = format!("{:.0}", self.panel.preview_fps)));
            } else {
                ui.strong(tr!("webcam-preview"));
            }
            if self.still
                && let Some((_, frame)) = fresh
            {
                if frame.detected() {
                    ui.colored_label(palette.success, tr!("webcam-still-held"));
                } else {
                    ui.colored_label(palette.muted, tr!("webcam-still-empty"));
                }
            } else if fresh.is_some_and(|(_, frame)| frame.detected()) {
                ui.colored_label(palette.success, tr!("webcam-fps", fps = format!("{:.0}", self.fps)));
            } else if self.panel.last_preview.is_some() {
                ui.colored_label(palette.muted, tr!("webcam-waiting"));
            }
            if fresh.is_some_and(|(_, frame)| frame.planar) {
                ui.colored_label(palette.muted, tr!("webcam-planar"));
            }
        });
        let empty = if self.error {
            tr!("webcam-preview-error")
        } else if self.running && matches!(self.panel.source, Source::Link(_)) && self.link_path.is_none() {
            tr!("webcam-link-fetching")
        } else if self.running {
            tr!("webcam-preview-opening")
        } else {
            tr!("webcam-preview-idle")
        };
        self.panel.preview(ui, palette, self.mirror, empty);
    }

    fn can_loop(&self) -> bool {
        !self.still && matches!(self.current_file_kind(), Some(FileKind::Video | FileKind::Gif))
    }

    fn current_file_kind(&self) -> Option<FileKind> {
        self.link_path
            .as_deref()
            .and_then(FileKind::of)
            .or_else(|| self.panel.source.file_kind())
    }

    fn controls(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        let can_loop = self.can_loop();
        ui.horizontal_wrapped(|ui| {
            widgets::checkbox(ui, palette, &mut self.panel.show_dots, tr!("webcam-landmarks"));
            widgets::checkbox(ui, palette, &mut self.mirror, tr!("webcam-mirror"));
            ui.add_enabled_ui(can_loop, |ui| {
                widgets::checkbox(ui, palette, &mut self.panel.loop_video, tr!("webcam-loop"));
            });
            self.front_view_requested |= ui.button(tr!("camera-settings-front-view")).clicked();
        });

        widgets::divider(ui, palette);
        let live = self.running
            && !self.still
            && self
                .latest
                .as_ref()
                .is_some_and(|(at, frame)| at.elapsed() < Duration::from_millis(250) && frame.detected());
        if self.recorder.ui(ui, palette, live) && self.running {
            self.stop();
        }

        widgets::divider(ui, palette);
        widgets::section_header(ui, palette, "bone-filled", &tr!("webcam-channels"));
        let ch = &mut self.panel.channels;
        ui.horizontal_wrapped(|ui| {
            widgets::checkbox(ui, palette, &mut ch.head, tr!("webcam-head"));
            widgets::checkbox(ui, palette, &mut ch.eyes, tr!("webcam-eyes"));
            widgets::checkbox(ui, palette, &mut ch.expression, tr!("webcam-expression"));
            widgets::checkbox(ui, palette, &mut ch.hands, tr!("webcam-hands"));
            widgets::checkbox(ui, palette, &mut ch.upper_body, tr!("webcam-upper-body"));
            widgets::checkbox(ui, palette, &mut ch.lower_body, tr!("webcam-lower-body"));
        });
        if ch.lower_body && self.running && !self.latest.as_ref().is_some_and(|(_, frame)| frame.legs_detected()) {
            ui.colored_label(palette.muted, tr!("webcam-lower-body-hint"));
        }
        ui.add_enabled_ui(ch.expression, |ui| {
            ui.add(egui::Slider::new(&mut ch.expression_strength, 0.0..=2.0).text(tr!("webcam-expression-strength")));
        });
        ui.colored_label(palette.muted, tr!("webcam-local"));
        if self.error {
            let detail = match self.link_error {
                Some(LinkError::Unsupported) => tr!("webcam-social-unsupported"),
                Some(LinkError::Dependencies) => tr!("webcam-social-dependencies"),
                Some(LinkError::Unavailable) => tr!("webcam-social-unavailable"),
                Some(LinkError::TooLarge) => tr!("webcam-link-too-large"),
                Some(LinkError::Download) => tr!("webcam-link-failed"),
                None => tr!("webcam-error"),
            };
            ui.colored_label(palette.danger, detail);
        }
    }

    fn capture_controls(&mut self, ui: &mut egui::Ui, palette: &Palette) {
        ui.horizontal_wrapped(|ui| {
            if self.running {
                let fresh = self.latest.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_millis(500));
                if ui.add_sized([124.0, 30.0], egui::Button::new(tr!("webcam-stop"))).clicked() {
                    self.stop();
                    return;
                }
                if ui
                    .add_enabled(
                        !self.still && fresh.is_some_and(|(_, f)| f.face.is_some() || (f.body[0].is_some() && f.body[3].is_some())),
                        egui::Button::new(tr!("webcam-recenter")),
                    )
                    .clicked()
                {
                    self.neutral = fresh.map(|(_, frame)| frame.clone());
                }
            } else {
                let label = match self.current_file_kind() {
                    Some(FileKind::Image) => tr!("webcam-track-image"),
                    Some(_) => tr!("webcam-replay"),
                    None => tr!("webcam-start"),
                };
                if widgets::primary_button_sized(ui, palette, label, egui::vec2(124.0, 30.0)).clicked() {
                    self.start();
                }
            }
            if self.current_file_kind() == Some(FileKind::Video) {
                let mut muted = self.video_muted.load(Ordering::Relaxed);
                if widgets::checkbox(ui, palette, &mut muted, tr!("webcam-mute-video")).changed() {
                    self.video_muted.store(muted, Ordering::Relaxed);
                }
            }
        });
    }
}
