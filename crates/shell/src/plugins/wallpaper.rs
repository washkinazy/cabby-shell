use crate::ui;
use cabby_config::{Config, WallpaperMode};
use iced::widget::{Space, container, image, responsive};
use iced::{ContentFit, Element, Length};
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
#[derive(Clone)]
pub(crate) struct Wallpaper {
    config: Config,
    handle: Option<image::Handle>,
    dimensions: iced::Size,
}
impl Wallpaper {
    pub(crate) fn new(config: Config) -> cabby_config::Result<Self> {
        let asset = cabby_config::wallpaper::load(&config)?;
        let dimensions = asset
            .as_ref()
            .map(|a| iced::Size::new(a.width as f32, a.height as f32))
            .unwrap_or(iced::Size::ZERO);
        let handle = asset.map(|a| image::Handle::from_rgba(a.width, a.height, a.rgba));
        Ok(Self {
            config,
            handle,
            dimensions,
        })
    }
    pub(crate) fn view<Message: 'static>(&self) -> Element<'_, Message> {
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
