use cabby_config::{Config, WallpaperMode};
use cabby_platform_spike::ui;
use iced::widget::{Space, container, image, responsive};
use iced::{ContentFit, Element, Length, Task, window};
use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, Layer, LayerSize};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::{daemon, to_exwlshell_message};
fn fit(mode: WallpaperMode) -> ContentFit {
    match mode {
        WallpaperMode::Fill => ContentFit::Cover,
        WallpaperMode::Fit => ContentFit::Contain,
        WallpaperMode::Stretch => ContentFit::Fill,
        WallpaperMode::Center => ContentFit::Contain,
    }
}
// Contain centers its draw bounds; undo only its scaling for native-size centering.
fn center_scale(source: iced::Size, bounds: iced::Size) -> f32 {
    (source.width / bounds.width.max(1.0)).max(source.height / bounds.height.max(1.0))
}
fn main() -> iced_exwlshell::Result {
    let config = ui::configuration_source()
        .and_then(|s| s.load(true))
        .unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1)
        });
    ui::renderer(&config);
    let asset = cabby_config::wallpaper::load(&config).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let dimensions = asset
        .as_ref()
        .map(|a| iced::Size::new(a.width as f32, a.height as f32))
        .unwrap_or(iced::Size::ZERO);
    let handle = asset.map(|a| image::Handle::from_rgba(a.width, a.height, a.rgba));
    cabby_platform_spike::renderer_diagnostics();
    iced_exwlshell::disable_clipboard();
    daemon(
        move || Wallpaper {
            config: config.clone(),
            handle: handle.clone(),
            dimensions,
        },
        || "cabby-wallpaper".into(),
        Wallpaper::update,
        Wallpaper::view,
    )
    // Rebuild the static view on output resize/scale events. Without a message,
    // the software renderer can retain image placement from the previous layout.
    .subscription(|_| iced::window::resize_events().map(|_| Message::Resized))
    .settings(Settings {
        layer_settings: LayerShellSettings {
            anchor: Anchor::all(),
            layer: Layer::Background,
            size: LayerSize::FILL,
            exclusive_zone: -1,
            keyboard_interactivity: KeyboardInteractivity::None,
            events_transparent: true,
            start_mode: StartMode::AllScreens,
            ..Default::default()
        },
        ..Default::default()
    })
    .run()
}
struct Wallpaper {
    config: Config,
    handle: Option<image::Handle>,
    dimensions: iced::Size,
}
#[to_exwlshell_message]
#[derive(Debug, Clone)]
enum Message {
    Resized,
}
impl Wallpaper {
    fn update(&mut self, _: Message) -> Task<Message> {
        Task::none()
    }
    fn view(&self, _: window::Id) -> Element<'_, Message> {
        let content: Element<'_, Message> = match &self.handle {
            Some(handle) => responsive(move |bounds| {
                image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(fit(self.config.wallpaper_mode))
                    .scale(if self.config.wallpaper_mode == WallpaperMode::Center {
                        center_scale(self.dimensions, bounds)
                    } else {
                        1.0
                    })
                    .into()
            })
            .into(),
            None => Space::new().width(Length::Fill).height(Length::Fill).into(),
        };
        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(|_| container::Style {
                background: Some(ui::color(&self.config.wallpaper_color).into()),
                ..Default::default()
            })
            .into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_geometry_preserves_or_stretches_as_requested() {
        let source = iced::Size::new(200.0, 100.0);
        let bounds = iced::Size::new(100.0, 100.0);
        assert_eq!(
            fit(WallpaperMode::Fill).fit(source, bounds),
            iced::Size::new(200.0, 100.0)
        );
        assert_eq!(
            fit(WallpaperMode::Fit).fit(source, bounds),
            iced::Size::new(100.0, 50.0)
        );
        assert_eq!(fit(WallpaperMode::Stretch).fit(source, bounds), bounds);
        assert_eq!(
            fit(WallpaperMode::Center).fit(source, bounds) * center_scale(source, bounds),
            source
        );
        let larger = iced::Size::new(800.0, 600.0);
        assert_eq!(
            fit(WallpaperMode::Center).fit(source, larger) * center_scale(source, larger),
            source
        );
        // Portrait and mixed logical sizes exercise output-shape changes.
        assert_eq!(
            fit(WallpaperMode::Fit).fit(source, iced::Size::new(60.0, 120.0)),
            iced::Size::new(60.0, 30.0)
        );
    }
}
