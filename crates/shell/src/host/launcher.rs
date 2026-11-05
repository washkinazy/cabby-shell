//! Active-output overlay hosting; closing a plugin is a host lifecycle decision.
use super::Context;
use crate::{
    plugins::launcher::{self, Action, Launcher},
    ui,
};
use cabby_config::Config;
use iced::{Element, Task, event, window};
use iced_exwlshell::reexport::{Anchor, KeyboardInteractivity, Layer, LayerSize};
use iced_exwlshell::settings::{LayerShellSettings, Settings, StartMode};
use iced_exwlshell::{daemon, to_exwlshell_message};

struct Overlay {
    config: Config,
    launcher: Launcher,
}
#[to_exwlshell_message]
#[derive(Clone, Debug)]
enum Message {
    Launcher(launcher::Message),
    Ready,
}
impl Overlay {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Ready => iced::widget::operation::focus("launcher-query"),
            Message::Launcher(message) => match self.launcher.update(message) {
                Action::Update(task) => task.map(Message::Launcher),
                Action::Close => iced::exit(),
            },
            _ => Task::none(),
        }
    }
    fn view(&self, _: window::Id) -> Element<'_, Message> {
        self.launcher.view().map(Message::Launcher)
    }
}
pub(super) fn run(context: Context) -> iced_exwlshell::Result {
    let Context { config, source } = context;
    let settings = Settings {
        default_text_size: iced::Pixels(f32::from(config.font_size)),
        layer_settings: LayerShellSettings {
            anchor: Anchor::empty(),
            layer: Layer::Overlay,
            size: LayerSize::px(
                u32::from(config.launcher_width),
                u32::from(config.launcher_rows + 3) * u32::from(config.font_size.max(24) + 20) + 64,
            ),
            exclusive_zone: 0,
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            start_mode: StartMode::Active,
            ..Default::default()
        },
        ..Default::default()
    };
    daemon(
        move || {
            let (launcher, task) =
                Launcher::new(config.clone(), source.explicit.then(|| source.path.clone()));
            (
                Overlay {
                    config: config.clone(),
                    launcher,
                },
                Task::batch([
                    task.map(Message::Launcher),
                    iced::widget::operation::focus("launcher-query"),
                ]),
            )
        },
        || "cabby-launcher".into(),
        Overlay::update,
        Overlay::view,
    )
    .on_new_shell(|_| Some(Message::Ready))
    .subscription(|_| event::listen_with(launcher::launcher_event).map(Message::Launcher))
    .theme(|state: &Overlay, _| ui::theme(&state.config))
    .settings(settings)
    .run()
}
