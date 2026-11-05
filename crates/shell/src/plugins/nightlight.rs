use serde::Deserialize;
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Status {
    state: String,
    phase: String,
    enabled: bool,
    force: bool,
    temperature: Option<u16>,
    outputs: usize,
    inhibited_outputs: usize,
    failed: usize,
}
impl Status {
    pub fn label(&self) -> &'static str {
        if self.inhibited_outputs > 0 {
            return "Night paused";
        }
        if !self.enabled && !self.force && self.phase == "off" {
            "Night off"
        } else if ["unsupported", "unavailable", "output-failed", "starting"]
            .contains(&self.state.as_str())
        {
            "Night light ?"
        } else {
            match self.phase.as_str() {
                "forced" => "Night forced",
                "night" => "Night on",
                "day" => "Night scheduled",
                "no-schedule" => "Night no schedule",
                _ => "Night light ?",
            }
        }
    }
    pub fn detail(&self) -> String {
        match self.temperature {
            Some(t) => format!(
                "Night light: {t} K; {} outputs, {} failed. Click to toggle schedule.",
                self.outputs, self.failed
            ),
            None => format!("Night light: {}. Click to toggle schedule.", self.state),
        }
    }
}
pub(super) fn events() -> impl iced::futures::Stream<Item = Status> {
    crate::runtime_status::events("nightlight.json")
}
