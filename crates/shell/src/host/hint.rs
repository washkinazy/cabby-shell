//! Passive bar hints live outside the bar's clipped Wayland surface.
use super::panel::Origin;
use cabby_config::{BarPosition, Config};
use iced::{Subscription, window};
use iced_exwlshell::reexport::{
    Anchor, KeyboardInteractivity, Layer, LayerSize, NewLayerShellSettings, OutputOption,
};
#[derive(Default)]
pub(super) struct Hint {
    pub serial: u64,
    pub pending: Option<(Origin, String)>,
    pub surface: Option<(window::Id, u32)>,
}
impl Hint {
    pub fn subscription(&self) -> Subscription<u64> {
        if self.pending.is_none() || self.surface.is_some() {
            return Subscription::none();
        }
        Subscription::run_with(self.serial, |serial| {
            let serial = *serial;
            iced::futures::stream::once(async move {
                tokio::time::sleep(std::time::Duration::from_millis(450)).await;
                serial
            })
        })
    }
}
pub(super) fn settings(
    config: &Config,
    size: (u32, u32),
    origin: Origin,
    label: &str,
) -> NewLayerShellSettings {
    let gap = 8u32.min(size.0 / 2).min(size.1 / 2);
    let font = u32::from(config.font_size);
    let width = (label.chars().count() as u32 * font * 3 / 4 + 24)
        .clamp(64, 360)
        .min(size.0.saturating_sub(2 * gap))
        .max(1);
    let columns = (width.saturating_sub(24) * 4 / (font * 3)).max(1);
    let lines = (label.chars().count() as u32).div_ceil(columns).max(1);
    let height = (lines * (font * 3 / 2) + 20)
        .min(
            size.1
                .saturating_sub(u32::from(config.bar_height) + 2 * gap),
        )
        .max(1);
    let left = (origin.x - width as f32 / 2.)
        .max(gap as f32)
        .min(size.0.saturating_sub(width + gap) as f32)
        .max(0.) as i32;
    let edge = (u32::from(config.bar_height) + gap).min(size.1.saturating_sub(height)) as i32;
    let top = config.bar_position == BarPosition::Top;
    NewLayerShellSettings {
        namespace: Some("cabby-bar-hint".into()),
        output_option: OutputOption::GlobalName(origin.output),
        layer: Layer::Overlay,
        anchor: Anchor::Left | if top { Anchor::Top } else { Anchor::Bottom },
        size: LayerSize::px(width, height),
        margin: Some(if top {
            (edge, 0, 0, left)
        } else {
            (0, 0, edge, left)
        }),
        exclusive_zone: Some(-1),
        keyboard_interactivity: KeyboardInteractivity::None,
        events_transparent: true,
        ..Default::default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hints_fit_both_bar_edges_and_small_outputs_without_taking_input() {
        for position in [BarPosition::Top, BarPosition::Bottom] {
            let config = Config {
                bar_position: position,
                ..Default::default()
            };
            for size in [(1920, 1080), (320, 200), (40, 20), (1, 1)] {
                let s = settings(
                    &config,
                    size,
                    Origin {
                        output: 1,
                        x: 10000.,
                    },
                    "Network unavailable",
                );
                assert!(s.events_transparent);
                assert_eq!(s.keyboard_interactivity, KeyboardInteractivity::None);
                let margin = s.margin.unwrap();
                let edge = if position == BarPosition::Top {
                    margin.0
                } else {
                    margin.2
                };
                let (width, height) = (s.size.width.to_set(), s.size.height.to_set());
                assert!(margin.3 >= 0 && margin.3 as u32 + width <= size.0);
                assert!(edge >= 0 && edge as u32 + height <= size.1);
            }
        }
    }
}
