//! Frozen-image selector and preview. Capture completes before selection UI renders.
use cabby_screenshot::{Error, Frame};
use iced::widget::{
    Space, button, column, container, image, mouse_area, row, stack, text, text_input,
};
use iced::{Element, Length, Point, Task};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone)]
pub(crate) enum Message {
    Begin,
    Captured(Result<Frame, Error>),
    Move(Point),
    Press,
    Release,
    Monitor,
    Reselect,
    Cropped(Result<Frame, Error>),
    Copy,
    Save,
    Done(&'static str, Result<(), String>),
    Filename(String),
    Close,
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScreenshotMessage")
    }
}
pub(crate) enum Action {
    Update(Task<Message>),
    Close,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Capturing,
    Selecting,
    Preview,
    Error,
}
pub(crate) struct Picker {
    root: PathBuf,
    output: String,
    pub logical: (u32, u32),
    monitor: bool,
    started: bool,
    mapped: bool,
    stage: Stage,
    source: Option<(Frame, image::Handle)>,
    selected: Option<(Frame, image::Handle)>,
    pointer: Point,
    start: Option<Point>,
    cancel: tokio::sync::watch::Sender<bool>,
    save_cancel: Arc<AtomicBool>,
    busy: bool,
    filename: String,
    error: Option<String>,
    action: &'static str,
    last: String,
}
fn handle(frame: &Frame) -> image::Handle {
    image::Handle::from_rgba(frame.width, frame.height, frame.rgba.as_ref().clone())
}
fn publish(root: &Path, value: &serde_json::Value) {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let temp = root.join("screenshot.tmp");
    let result = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temp)?;
        write!(f, "{value}")?;
        std::fs::rename(&temp, root.join("screenshot.json"))
    })();
    if result.is_err() {
        eprintln!("Screenshot status unavailable");
    }
}
pub(crate) fn idle(root: &Path) {
    publish(
        root,
        &serde_json::json!({"state":"idle","pid":std::process::id()}),
    );
}
impl Drop for Picker {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
        self.save_cancel.store(true, Ordering::Release);
        idle(&self.root);
    }
}
impl Picker {
    pub fn new(root: PathBuf, output: String, logical: (u32, u32), monitor: bool) -> Self {
        let (cancel, _) = tokio::sync::watch::channel(false);
        let mut p = Self {
            root,
            output,
            logical,
            monitor,
            started: false,
            mapped: false,
            stage: Stage::Capturing,
            source: None,
            selected: None,
            pointer: Point::ORIGIN,
            start: None,
            cancel,
            save_cancel: Arc::new(AtomicBool::new(false)),
            busy: false,
            filename: cabby_screenshot::filename(),
            error: None,
            action: "ready",
            last: String::new(),
        };
        p.notify();
        p
    }
    pub fn mapped(&self) -> bool {
        self.mapped
    }
    pub fn mark_mapped(&mut self) {
        self.mapped = true;
    }
    pub fn capturing(&self) -> bool {
        self.stage == Stage::Capturing
    }
    pub fn fullscreen(&self) -> bool {
        matches!(self.stage, Stage::Capturing | Stage::Selecting)
    }
    fn notify(&mut self) {
        let value = serde_json::json!({"state":match self.stage{Stage::Capturing=>"capturing",Stage::Selecting=>"selecting",Stage::Preview=>"preview",Stage::Error=>"error"},"pid":std::process::id(),"busy":self.busy,"action":self.action,"error":self.error,"logical_size":[self.logical.0,self.logical.1],"source_size":self.source.as_ref().map(|(f,_)|[f.width,f.height]),"selected_size":self.selected.as_ref().map(|(f,_)|[f.width,f.height])});
        let s = value.to_string();
        if s != self.last {
            publish(&self.root, &value);
            self.last = s;
        }
    }
    pub fn update(&mut self, message: Message, config: &cabby_config::Config) -> Action {
        let task = match message {
            Message::Begin if self.stage == Stage::Capturing && !self.started => {
                self.started = true;
                self.busy = true;
                let output = self.output.clone();
                let cursor = config.screenshot.include_cursor;
                let cancel = self.cancel.subscribe();
                Task::perform(
                    cabby_screenshot::capture(output, cursor, cancel),
                    Message::Captured,
                )
            }
            Message::Captured(Ok(frame)) => {
                self.busy = false;
                let texture = handle(&frame);
                self.source = Some((frame.clone(), texture.clone()));
                if self.monitor {
                    self.selected = Some((frame, texture));
                    self.stage = Stage::Preview;
                } else {
                    self.stage = Stage::Selecting;
                }
                Task::none()
            }
            Message::Captured(Err(error)) | Message::Cropped(Err(error)) => {
                self.busy = false;
                self.error = Some(error.to_string());
                if self.source.is_none() {
                    self.stage = Stage::Error;
                }
                Task::none()
            }
            Message::Move(point) if self.stage == Stage::Selecting && !self.busy => {
                self.pointer = Point::new(
                    point.x.clamp(0., self.logical.0 as f32),
                    point.y.clamp(0., self.logical.1 as f32),
                );
                Task::none()
            }
            Message::Press if self.stage == Stage::Selecting && !self.busy => {
                self.start = Some(self.pointer);
                self.error = None;
                Task::none()
            }
            Message::Release if self.stage == Stage::Selecting && !self.busy => {
                let Some(start) = self.start.take() else {
                    return Action::Update(Task::none());
                };
                let Some((frame, _)) = &self.source else {
                    return Action::Update(Task::none());
                };
                match cabby_screenshot::region(
                    self.logical,
                    (frame.width, frame.height),
                    (start.x, start.y),
                    (self.pointer.x, self.pointer.y),
                ) {
                    Ok(r)
                        if (start.x - self.pointer.x).abs() >= 2.
                            && (start.y - self.pointer.y).abs() >= 2. =>
                    {
                        self.busy = true;
                        Task::perform(
                            cabby_screenshot::crop_region(
                                frame.clone(),
                                r,
                                self.cancel.subscribe(),
                            ),
                            Message::Cropped,
                        )
                    }
                    _ => {
                        self.error = Some(Error::InvalidSelection.to_string());
                        Task::none()
                    }
                }
            }
            Message::Monitor if !self.busy => {
                if let Some(source) = &self.source {
                    self.selected = Some(source.clone());
                    self.stage = Stage::Preview;
                    self.error = None;
                }
                Task::none()
            }
            Message::Reselect if !self.busy && self.source.is_some() => {
                self.selected = None;
                self.stage = Stage::Selecting;
                self.start = None;
                self.error = None;
                self.action = "ready";
                Task::none()
            }
            Message::Cropped(Ok(frame)) => {
                self.busy = false;
                self.selected = Some((frame.clone(), handle(&frame)));
                self.stage = Stage::Preview;
                self.action = "ready";
                Task::none()
            }
            Message::Filename(name) if !self.busy => {
                self.filename = name.chars().take(255).collect();
                self.action = "ready";
                self.error = None;
                Task::none()
            }
            Message::Copy if !self.busy => {
                if let Some((frame, _)) = &self.selected {
                    self.busy = true;
                    let png = frame.png.clone();
                    let root = self.root.clone();
                    Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                cabby_clipboard::copy_png(&root, &png)
                            })
                            .await
                            .unwrap_or_else(|_| Err("Clipboard image transfer failed".into()))
                        },
                        |r| Message::Done("copied", r),
                    )
                } else {
                    Task::none()
                }
            }
            Message::Save if !self.busy => {
                if let Some((frame, _)) = &self.selected {
                    self.busy = true;
                    let frame = frame.clone();
                    let directory = directory(&config.screenshot.directory);
                    let filename = self.filename.clone();
                    let cancel = self.save_cancel.clone();
                    Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                let directory = directory?;
                                cabby_screenshot::save(&frame, &directory, &filename, &cancel)
                                    .map(|_| ())
                                    .map_err(|e| e.to_string())
                            })
                            .await
                            .unwrap_or_else(|_| Err("Screenshot save failed".into()))
                        },
                        |r| Message::Done("saved", r),
                    )
                } else {
                    Task::none()
                }
            }
            Message::Done(action, result) => {
                self.busy = false;
                match result {
                    Ok(()) => {
                        self.error = None;
                        self.action = action;
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.action = "error";
                    }
                }
                Task::none()
            }
            Message::Close => return Action::Close,
            _ => Task::none(),
        };
        self.notify();
        Action::Update(task)
    }
    pub fn view(&self, config: &cabby_config::Config) -> Element<'_, Message> {
        let size = f32::from(config.font_size);
        let label = |value: &'static str| text(value).size(size);
        let control_height = (size * 1.3 + 10.).ceil().max(32.);
        if self.stage == Stage::Capturing {
            return Space::new().width(Length::Fill).height(Length::Fill).into();
        }
        if self.stage == Stage::Selecting {
            let Some((_, texture)) = &self.source else {
                return Space::new().into();
            };
            let picture = image(texture.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Fill);
            let mut layers = stack![picture];
            if let Some(start) = self.start {
                let x = start.x.min(self.pointer.x);
                let y = start.y.min(self.pointer.y);
                let width = (start.x - self.pointer.x).abs().max(1.);
                let height = (start.y - self.pointer.y).abs().max(1.);
                let border = container(Space::new()).width(width).height(height).style(
                    |theme: &iced::Theme| container::Style {
                        border: iced::Border {
                            color: theme.palette().primary,
                            width: 2.,
                            radius: 0.into(),
                        },
                        background: Some(iced::Color::from_rgba(0., 0., 0., 0.12).into()),
                        ..Default::default()
                    },
                );
                layers = layers.push(
                    container(column![
                        Space::new().height(y),
                        row![Space::new().width(x), border]
                    ])
                    .width(Length::Fill)
                    .height(Length::Fill),
                );
            }
            let description = if self.busy {
                "Preparing region…"
            } else {
                self.error
                    .as_deref()
                    .unwrap_or("Drag a region, or choose the whole monitor")
            };
            let monitor = button(label("Monitor"))
                .width((size * 4.6 + 16.).max(100.))
                .height(control_height);
            let cancel = button(label("Cancel"))
                .width((size * 4. + 16.).max(90.))
                .height(control_height)
                .on_press(Message::Close);
            let toolbar = container(
                row![
                    text(description).size(size).width(Length::Fill),
                    if self.busy {
                        monitor
                    } else {
                        monitor.on_press(Message::Monitor)
                    },
                    cancel
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            )
            .padding(16)
            .width(Length::Fill)
            .style(container::dark);
            layers = layers.push(
                container(toolbar)
                    .height(Length::Fill)
                    .align_y(iced::alignment::Vertical::Bottom),
            );
            return mouse_area(layers)
                .interaction(iced::mouse::Interaction::Crosshair)
                .into();
        }
        let mut body = column![row![
            text("Screenshot").size(24).width(Length::Fill),
            button(label("Select region"))
                .on_press_maybe((!self.busy && self.source.is_some()).then_some(Message::Reselect))
        ]]
        .spacing(12);
        if let Some(error) = &self.error {
            body = body.push(text(error).size(size));
        } else if self.action == "saved" {
            body = body.push(label("Saved"));
        } else if self.action == "copied" {
            body = body.push(label("Copied to clipboard"));
        }
        body = body.push(
            text_input("PNG filename", &self.filename)
                .size(size)
                .on_input(Message::Filename),
        );
        if let Some((frame, texture)) = &self.selected {
            body = body
                .push(text(format!("{} × {}", frame.width, frame.height)).size(size))
                .push(
                    image(texture.clone())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .content_fit(iced::ContentFit::Contain),
                );
        } else {
            body = body.push(Space::new().height(Length::Fill));
        }
        let mut copy = button(label("Copy"))
            .width(Length::Fill)
            .height(control_height);
        let mut save = button(label("Save"))
            .width(Length::Fill)
            .height(control_height);
        if !self.busy && self.selected.is_some() {
            copy = copy.on_press(Message::Copy);
            save = save.on_press(Message::Save);
        }
        body = body.push(
            row![
                copy,
                save,
                button(label("Cancel"))
                    .on_press(Message::Close)
                    .width(Length::Fill)
                    .height(control_height)
            ]
            .spacing(8),
        );
        container(body)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container::rounded_box)
            .into()
    }
}
fn directory(configured: &str) -> Result<PathBuf, String> {
    if configured.is_empty() {
        std::env::var_os("HOME")
            .map(|p| PathBuf::from(p).join("Pictures/Screenshots"))
            .ok_or_else(|| "Screenshot directory unavailable".into())
    } else {
        Ok(PathBuf::from(configured))
    }
}
