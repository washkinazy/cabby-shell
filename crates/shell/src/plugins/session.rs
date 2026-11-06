//! On-demand session controls; power/session policy stays in the Cabby CLI.
use crate::ui::{
    self,
    symbol::{self, Symbol},
};
use cabby_config::Config;
use iced::widget::{Space, column, container, row, scrollable};
use iced::{Element, Length, Task};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Lock,
    Suspend,
    Logout,
    Reboot,
    PowerOff,
}
impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lock => "Lock",
            Self::Suspend => "Suspend",
            Self::Logout => "Log out",
            Self::Reboot => "Reboot",
            Self::PowerOff => "Power off",
        }
    }
    fn command(self) -> &'static str {
        match self {
            Self::Lock => "lock",
            Self::Suspend => "suspend",
            Self::Logout => "logout",
            Self::Reboot => "reboot",
            Self::PowerOff => "poweroff",
        }
    }
    fn icon(self) -> Symbol {
        match self {
            Self::Lock => Symbol::Lock,
            Self::Suspend => Symbol::Suspend,
            Self::Logout => Symbol::Logout,
            Self::Reboot => Symbol::Reboot,
            Self::PowerOff => Symbol::Power,
        }
    }
    fn confirms(self) -> bool {
        matches!(self, Self::Logout | Self::Reboot | Self::PowerOff)
    }
}
#[derive(Clone, Debug)]
pub(crate) enum Message {
    Move(i32),
    Activate,
    Choose(Action),
    Confirm,
    Back,
    Close,
    Done(Result<(), String>),
}
#[derive(Default)]
pub(crate) struct Menu {
    cursor: usize,
    confirm_selected: bool,
    selection: Option<Action>,
    pending: Option<iced::task::Handle>,
    error: bool,
}
impl Menu {
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn update(&mut self, message: Message) -> (Task<Message>, bool) {
        match message {
            Message::Close => return (Task::none(), true),
            Message::Done(result) if self.pending.is_some() => {
                self.pending.take();
                self.error = result.is_err();
                return (Task::none(), result.is_ok());
            }
            _ if self.pending.is_some() => (),
            Message::Move(delta) => {
                if self.selection.is_some_and(Action::confirms) {
                    self.confirm_selected = !self.confirm_selected;
                } else {
                    self.cursor = (self.cursor as i32 + delta).rem_euclid(5) as usize;
                }
            }
            Message::Activate => {
                let message = if self.selection.is_some_and(Action::confirms) {
                    if self.confirm_selected {
                        Message::Confirm
                    } else {
                        Message::Back
                    }
                } else {
                    Message::Choose(
                        [
                            Action::Lock,
                            Action::Suspend,
                            Action::Logout,
                            Action::Reboot,
                            Action::PowerOff,
                        ][self.cursor],
                    )
                };
                return self.update(message);
            }
            Message::Back => {
                self.selection = None;
                self.error = false;
            }
            Message::Choose(action) => {
                self.selection = Some(action);
                self.confirm_selected = false;
                self.cursor = [
                    Action::Lock,
                    Action::Suspend,
                    Action::Logout,
                    Action::Reboot,
                    Action::PowerOff,
                ]
                .iter()
                .position(|a| *a == action)
                .unwrap();
                self.error = false;
                if !action.confirms() {
                    return (self.run(action), false);
                }
            }
            Message::Confirm => {
                if let Some(action) = self.selection {
                    return (self.run(action), false);
                }
            }
            _ => (),
        }
        (Task::none(), false)
    }
    fn run(&mut self, action: Action) -> Task<Message> {
        let (task, handle) = Task::perform(
            ui::command_with_timeout(
                vec![action.command().into()],
                std::time::Duration::from_secs(60),
            ),
            Message::Done,
        )
        .abortable();
        self.pending = Some(handle.abort_on_drop());
        task
    }
    pub fn view(&self, c: &Config) -> Element<'_, Message> {
        let height = (f32::from(c.font_size) + 20.).max(44.);
        let title = self
            .selection
            .filter(|a| a.confirms())
            .map(|a| format!("{}?", a.label()))
            .unwrap_or_else(|| "Session".into());
        let header = row![
            ui::heading(c, title).width(Length::Fill),
            ui::button("Close")
                .style(ui::quiet)
                .on_press(Message::Close)
        ]
        .spacing(8);
        let mut body = column![header].spacing(12);
        if self.pending() {
            body = body.push(ui::body(c, "Requesting…"));
        } else if let Some(action) = self.selection.filter(|a| a.confirms()) {
            body = body
                .push(ui::body(
                    c,
                    match action {
                        Action::Logout => "End the graphical session? Unsaved work may be lost.",
                        Action::Reboot => "Restart this computer? Unsaved work may be lost.",
                        _ => "Shut down this computer? Unsaved work may be lost.",
                    },
                ))
                .push(Space::new().height(Length::Fill));
            body = body.push(
                row![
                    ui::button("Cancel")
                        .height(height)
                        .width(Length::Fill)
                        .style(if self.confirm_selected {
                            ui::standard
                        } else {
                            ui::selected
                        })
                        .on_press(Message::Back),
                    ui::button(action.label())
                        .height(height)
                        .width(Length::Fill)
                        .style(ui::primary)
                        .on_press(Message::Confirm)
                ]
                .spacing(8),
            );
        } else {
            let mut actions = column![].spacing(8);
            for (index, action) in [
                Action::Lock,
                Action::Suspend,
                Action::Logout,
                Action::Reboot,
                Action::PowerOff,
            ]
            .into_iter()
            .enumerate()
            {
                actions = actions.push(
                    ui::button(
                        row![
                            symbol::view(action.icon().handle()),
                            ui::body(c, action.label())
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                    )
                    .height(height)
                    .width(Length::Fill)
                    .style(if index == self.cursor {
                        ui::selected
                    } else {
                        ui::quiet
                    })
                    .on_press(Message::Choose(action)),
                );
            }
            body = body.push(scrollable(actions).height(Length::Fill));
            body = body.push(ui::secondary(c, "↑/↓ choose · Enter select · Esc close"));
        }
        if self.error {
            body=body.push(ui::body(c,if self.selection==Some(Action::Suspend) {"Suspend was rejected or the lock could not be confirmed. Check Caffeine and lock support."} else {"The request failed. Check session status before retrying."}));
        }
        container(body)
            .padding(16)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(ui::panel(c.renderer))
            .into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_confirmation_defaults_to_cancel() {
        let mut menu = Menu::default();
        let _ = menu.update(Message::Move(3));
        let _ = menu.update(Message::Activate);
        assert_eq!(menu.selection, Some(Action::Reboot));
        assert!(!menu.pending());
        let _ = menu.update(Message::Activate);
        assert_eq!(menu.selection, None);
        assert!(!menu.pending());
        let _ = menu.update(Message::Activate);
        let _ = menu.update(Message::Move(1));
        let (task, _) = menu.update(Message::Activate);
        assert!(menu.pending());
        drop(task);
    }
    #[test]
    fn destructive_choices_require_confirmation_and_pending_actions_are_coalesced() {
        let mut menu = Menu::default();
        for action in [Action::Logout, Action::Reboot, Action::PowerOff] {
            let _ = menu.update(Message::Choose(action));
            assert!(!menu.pending());
            let _ = menu.update(Message::Back);
            assert_eq!(menu.selection, None);
        }
        let _ = menu.update(Message::Confirm);
        assert!(!menu.pending());
        let _ = menu.update(Message::Choose(Action::Reboot));
        let (task, _) = menu.update(Message::Confirm);
        assert!(menu.pending());
        let _ = menu.update(Message::Choose(Action::PowerOff));
        assert_eq!(menu.selection, Some(Action::Reboot));
        let _ = menu.update(Message::Done(Err("fixture".into())));
        assert!(menu.error && !menu.pending());
        drop(task);
        for action in [Action::Lock, Action::Suspend] {
            let (task, _) = menu.update(Message::Choose(action));
            assert!(menu.pending());
            let (_, close) = menu.update(Message::Done(Ok(())));
            assert!(close);
            drop(task);
        }
    }
}
