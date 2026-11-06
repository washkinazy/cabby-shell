use serde::Deserialize;
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Status {
    active: bool,
    manual: bool,
    reasons: Vec<String>,
    sleep_state: String,
    audio_state: String,
    fullscreen_state: String,
}
impl Status {
    pub fn style(
        &self,
        failed: bool,
    ) -> fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
        if failed || self.active && self.sleep_state == "unavailable" {
            crate::ui::danger
        } else if self.manual {
            crate::ui::primary
        } else if self.active {
            crate::ui::selected
        } else {
            crate::ui::quiet
        }
    }
    pub fn icon(&self) -> iced::widget::svg::Handle {
        // A repository-owned vector keeps the indicator compact and avoids requiring
        // an icon/emoji font. Steam distinguishes active from idle at a fixed width.
        let steam = if self.active {
            r#"<path d="M4 4V2m4 2V1m4 3V2"/>"#
        } else {
            ""
        };
        iced::widget::svg::Handle::from_memory(format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 18 18"><g fill="none" stroke="white" stroke-width="1.5" stroke-linecap="round"><path d="M2 6h11v6a3 3 0 0 1-3 3H5a3 3 0 0 1-3-3ZM13 7h1a2 2 0 0 1 0 4h-1M1 17h14"/>{steam}</g></svg>"#).into_bytes())
    }
    pub fn detail(&self) -> String {
        format!(
            "Caffeine {}. Reasons: {}. Sleep: {}. Fullscreen: {}. Audio: {}. Click to toggle manual caffeine.",
            if self.manual {
                "manual"
            } else if self.active {
                "automatic"
            } else {
                "off"
            },
            if self.reasons.is_empty() {
                "none".into()
            } else {
                self.reasons.join(", ")
            },
            self.sleep_state,
            self.fullscreen_state,
            self.audio_state
        )
    }
}
pub(super) fn events() -> impl iced::futures::Stream<Item = Status> {
    crate::runtime_status::events("caffeine.json")
}
