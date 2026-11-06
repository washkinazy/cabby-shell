//! Transient feedback policy, independent of the host's passive surface lifecycle.
use super::lock_keys::{self, Keys};
use cabby_audio::{Kind, Snapshot};
use cabby_config::{Config, Osd};
use iced::widget::{column, container, progress_bar, row, svg, text};
use iced::{Element, Length, Subscription};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Content {
    Volume {
        input: bool,
        percent: u32,
        muted: bool,
    },
    Lock {
        caps: bool,
        on: bool,
    },
}
impl Content {
    fn enabled(&self, c: &Osd) -> bool {
        c.enabled
            && match self {
                Self::Volume { input: false, .. } => c.volume,
                Self::Volume { input: true, .. } => c.microphone,
                Self::Lock { caps: true, .. } => c.caps_lock,
                Self::Lock { caps: false, .. } => c.num_lock,
            }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Volume { input: false, .. } => "Volume",
            Self::Volume { input: true, .. } => "Microphone",
            Self::Lock { caps: true, .. } => "Caps Lock",
            Self::Lock { caps: false, .. } => "Num Lock",
        }
    }
    fn value(&self) -> String {
        match self {
            Self::Volume { muted: true, .. } => "Muted".into(),
            Self::Volume { percent, .. } => format!("{percent}%"),
            Self::Lock { on, .. } => if *on { "On" } else { "Off" }.into(),
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) enum Message {
    Keys(Keys),
    Expire(u64),
}
type AudioValue = Option<(String, u32, bool)>;
#[derive(Default)]
pub(crate) struct Feedback {
    pub active: Option<Content>,
    pub serial: u64,
    audio: [AudioValue; 2],
    caps: Option<bool>,
    num: Option<bool>,
}
impl Feedback {
    pub fn audio(&mut self, snapshot: Option<&Snapshot>, config: &Osd) -> Option<Content> {
        let mut event = None;
        for (index, kind) in [Kind::Output, Kind::Input].into_iter().enumerate() {
            let next = snapshot
                .and_then(|s| s.devices.iter().find(|d| d.kind == kind && s.is_default(d)))
                .map(|d| (d.name.clone(), d.percent, d.muted));
            if let (Some((old_name, old_percent, old_muted)), Some((name, percent, muted))) =
                (&self.audio[index], &next)
                && old_name == name
                && (old_percent != percent || old_muted != muted)
            {
                let value = Content::Volume {
                    input: index == 1,
                    percent: *percent,
                    muted: *muted,
                };
                if value.enabled(config) {
                    event = Some(value);
                }
            }
            self.audio[index] = next;
        }
        event
    }
    pub fn keys(&mut self, value: Keys, config: &Osd) -> Option<Content> {
        if value.reset {
            self.caps = None;
            self.num = None;
        }
        let mut event = None;
        for (previous, next, caps) in [
            (&mut self.caps, value.caps, true),
            (&mut self.num, value.num, false),
        ] {
            if let (Some(old), Some(on)) = (*previous, next)
                && old != on
            {
                let value = Content::Lock { caps, on };
                if value.enabled(config) {
                    event = Some(value);
                }
            }
            *previous = next;
        }
        event
    }
    pub fn show(&mut self, value: Content) {
        self.serial = self.serial.wrapping_add(1);
        self.active = Some(value);
    }
    pub fn expire(&mut self, serial: u64) -> bool {
        if serial != self.serial {
            return false;
        }
        self.active.take().is_some()
    }
    pub fn configure(&mut self, old: &Osd, new: &Osd) {
        if old.timeout_ms != new.timeout_ms && self.active.is_some() {
            self.serial = self.serial.wrapping_add(1);
        }
        // Existing audio subscribers keep observing while a category is hidden;
        // retaining that baseline must not swallow the first change on reenable.
        if self.active.as_ref().is_some_and(|v| !v.enabled(new)) {
            self.active = None;
            self.serial = self.serial.wrapping_add(1);
        }
    }
    pub fn subscription(&self, config: &Osd, leds: &std::path::Path) -> Subscription<Message> {
        let keys = if config.enabled && (config.caps_lock || config.num_lock) {
            Subscription::run_with(
                lock_keys::Settings {
                    root: leds.into(),
                    caps: config.caps_lock,
                    num: config.num_lock,
                },
                lock_keys::events,
            )
            .map(Message::Keys)
        } else {
            Subscription::none()
        };
        let timeout = if self.active.is_some() {
            Subscription::run_with((self.serial, config.timeout_ms), |(serial, ms)| {
                let (serial, ms) = (*serial, *ms);
                iced::futures::stream::once(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
                    Message::Expire(serial)
                })
            })
        } else {
            Subscription::none()
        };
        Subscription::batch([keys, timeout])
    }
    pub fn view(&self, config: &Config) -> Element<'_, Message> {
        let Some(value) = &self.active else {
            return iced::widget::Space::new().into();
        };
        let icon = match value {
            Content::Volume {
                input: true,
                muted: true,
                ..
            } => {
                "<rect x='7' y='2' width='6' height='12' rx='3'/><path d='M4 10v1a6 6 0 0 0 12 0v-1M10 17v3M6 20h8M2 2l18 18'/>"
            }
            Content::Volume { input: true, .. } => {
                "<rect x='7' y='2' width='6' height='12' rx='3'/><path d='M4 10v1a6 6 0 0 0 12 0v-1M10 17v3M6 20h8'/>"
            }
            Content::Volume { muted: true, .. } => {
                "<path d='M2 8h4l5-4v14l-5-4H2zM14 8l5 6M19 8l-5 6'/>"
            }
            Content::Volume { .. } => {
                "<path d='M2 8h4l5-4v14l-5-4H2zM14 7q5 4 0 8M17 4q8 7 0 14'/>"
            }
            Content::Lock { .. } => "<path d='M3 10l7-7 7 7h-4v7H7v-7zM7 20h6'/>",
        };
        let handle = svg::Handle::from_memory(format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 22 22' fill='none' stroke='white' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'>{icon}</svg>").into_bytes());
        let heading = row![
            svg(handle)
                .width(28)
                .height(28)
                .style(|theme: &iced::Theme, _| svg::Style {
                    color: Some(theme.palette().text)
                }),
            crate::ui::heading(config, value.label()).width(Length::Fill),
            text(value.value()).size(f32::from(config.font_size) + 3.)
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center);
        let mut contents = column![heading].spacing(12);
        if let Content::Volume { percent, muted, .. } = value {
            contents = contents.push(
                progress_bar(
                    0.0..=100.0,
                    if *muted {
                        0.0
                    } else {
                        (*percent).min(100) as f32
                    },
                )
                .girth(6)
                .style(|theme: &iced::Theme| progress_bar::Style {
                    background: crate::ui::neutral_track(theme).into(),
                    bar: theme.palette().primary.into(),
                    border: iced::Border {
                        radius: 3.into(),
                        ..Default::default()
                    },
                }),
            );
        }
        container(contents)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .style(crate::ui::panel(config.renderer))
            .into()
    }
}
pub(crate) struct Surface {
    pub id: iced::window::Id,
    pub output: u32,
}
pub(crate) fn geometry(config: &Config, size: (u32, u32)) -> (u32, u32, i32) {
    let width = u32::from(config.font_size)
        .saturating_mul(14)
        .max(320)
        .min(size.0.saturating_sub(16).max(1));
    let height = (u32::from(config.font_size) * 3 + 44)
        .max(100)
        .min(size.1.max(1));
    let clearance = if config.bar_position == cabby_config::BarPosition::Bottom {
        u32::from(config.bar_height) + 12
    } else {
        0
    };
    let bottom = u32::from(config.osd.bottom_margin)
        .max(clearance)
        .min(size.1.saturating_sub(height));
    (width, height, bottom as i32)
}
pub(crate) fn default_leds() -> PathBuf {
    "/sys/class/leds".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_reconnect_and_unrelated_audio_changes_are_silent() {
        let c = Osd::default();
        let mut f = Feedback::default();
        let mut s = Snapshot {
            default_output: Some("speakers".into()),
            devices: vec![cabby_audio::Device {
                kind: Kind::Output,
                name: "speakers".into(),
                description: "".into(),
                percent: 50,
                muted: false,
            }],
            ..Default::default()
        };
        assert_eq!(f.audio(Some(&s), &c), None);
        s.active = true;
        assert_eq!(f.audio(Some(&s), &c), None);
        s.devices[0].percent = 125;
        let event = f.audio(Some(&s), &c).unwrap();
        assert_eq!(event.value(), "125%");
        f.show(event);
        let first = f.serial;
        s.devices[0].muted = true;
        let next = f.audio(Some(&s), &c).unwrap();
        f.show(next);
        assert!(!f.expire(first));
        assert_eq!(f.active.as_ref().unwrap().value(), "Muted");
        let previous = f.serial;
        let mut longer = c.clone();
        longer.timeout_ms = 2500;
        f.configure(&c, &longer);
        assert!(
            !f.expire(previous),
            "configuration must invalidate the old expiry"
        );
        assert_eq!(f.audio(None, &c), None);
        assert_eq!(f.audio(Some(&s), &c), None);
        s.devices[0].name = "new device".into();
        s.default_output = Some("new device".into());
        assert_eq!(f.audio(Some(&s), &c), None);
        let mut off = c.clone();
        off.enabled = false;
        f.configure(&c, &off);
        assert!(f.active.is_none());
        s.devices[0].percent = 20;
        assert_eq!(f.audio(Some(&s), &off), None);
        f.configure(&off, &c);
        s.devices[0].percent = 30;
        assert!(f.audio(Some(&s), &c).is_some());
    }
    #[test]
    fn lock_state_changes_replace_feedback_but_baselines_and_loss_do_not() {
        let c = Osd::default();
        let mut f = Feedback::default();
        assert_eq!(
            f.keys(
                Keys {
                    caps: Some(false),
                    reset: true,
                    ..Default::default()
                },
                &c
            ),
            None
        );
        let next = f
            .keys(
                Keys {
                    caps: Some(true),
                    ..Default::default()
                },
                &c,
            )
            .unwrap();
        assert_eq!(next.value(), "On");
        f.show(next);
        assert!(f.expire(f.serial));
        assert_eq!(f.keys(Keys::default(), &c), None);
        assert_eq!(
            f.keys(
                Keys {
                    caps: Some(false),
                    ..Default::default()
                },
                &c
            ),
            None
        );
        assert_eq!(
            f.keys(
                Keys {
                    caps: Some(true),
                    reset: true,
                    ..Default::default()
                },
                &c
            ),
            None
        );
        assert_eq!(
            f.keys(
                Keys {
                    caps: Some(true),
                    num: Some(true),
                    ..Default::default()
                },
                &c
            ),
            None
        );
        for position in [
            cabby_config::BarPosition::Top,
            cabby_config::BarPosition::Bottom,
        ] {
            for size in [(1920, 1080), (320, 400), (80, 50), (1, 1)] {
                let config = Config {
                    bar_position: position,
                    ..Default::default()
                };
                let (w, h, y) = geometry(&config, size);
                assert!(w <= size.0 && h <= size.1 && y >= 0 && y as u32 + h <= size.1);
            }
        }
    }
}
