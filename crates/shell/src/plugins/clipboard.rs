//! Clipboard picker; payloads stay in the coordinator, previews only in this view.
use cabby_clipboard::{Request, Snapshot};
use iced::widget::{column, row, scrollable, text, text_input};
use iced::{Element, Task};
use std::path::PathBuf;
#[derive(Clone)]
pub(crate) enum Message {
    Query(String),
    Debounced(u64),
    Loaded(u64, Result<Snapshot, String>),
    Select(u64),
    Delete(u64),
    Clear,
    Done(bool, Result<(), String>),
    Refresh,
    Move(i32),
    Accept,
    Close,
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClipboardMessage")
    }
}
pub(crate) enum Action {
    Update(Task<Message>),
    Close,
}
pub(crate) struct Picker {
    root: PathBuf,
    query: String,
    state: Option<Snapshot>,
    error: Option<String>,
    generation: u64,
    busy: bool,
    selected: usize,
}
impl Picker {
    pub fn new(root: PathBuf) -> (Self, Task<Message>) {
        let mut s = Self {
            root,
            query: String::new(),
            state: None,
            error: None,
            generation: 0,
            busy: false,
            selected: 0,
        };
        let task = s.refresh();
        (s, task)
    }
    fn refresh(&mut self) -> Task<Message> {
        self.generation = self.generation.wrapping_add(1);
        let id = self.generation;
        let root = self.root.clone();
        let request = Request::List {
            query: self.query.clone(),
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let value = cabby_clipboard::client(&root, &request)?;
                    serde_json::from_value(value["clipboard"].clone())
                        .map_err(|_| "clipboard response unavailable".into())
                })
                .await
                .unwrap_or_else(|_| Err("clipboard unavailable".into()))
            },
            move |r| Message::Loaded(id, r),
        )
    }
    pub fn update(&mut self, message: Message) -> Action {
        let task = match message {
            Message::Query(query) => {
                self.query = query.chars().take(256).collect();
                while self.query.len() > 256 {
                    self.query.pop();
                }
                self.selected = 0;
                self.generation = self.generation.wrapping_add(1);
                let id = self.generation;
                Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        id
                    },
                    Message::Debounced,
                )
            }
            Message::Debounced(id) if id == self.generation => self.refresh(),
            Message::Loaded(id, result) if id == self.generation => {
                match result {
                    Ok(s) => {
                        self.state = Some(s);
                        self.error = None;
                        self.selected = self
                            .selected
                            .min(self.state.as_ref().unwrap().entries.len().saturating_sub(1));
                    }
                    Err(e) => {
                        self.state = None;
                        self.error = Some(e);
                    }
                }
                Task::none()
            }
            Message::Refresh if !self.busy => self.refresh(),
            Message::Move(direction) => {
                let len = self.state.as_ref().map_or(0, |s| s.entries.len());
                if len > 0 {
                    self.selected =
                        (self.selected as i32 + direction).rem_euclid(len as i32) as usize;
                }
                Task::none()
            }
            Message::Accept if !self.busy => {
                if let Some(id) = self
                    .state
                    .as_ref()
                    .and_then(|s| s.entries.get(self.selected))
                    .map(|i| i.id)
                {
                    return self.update(Message::Select(id));
                }
                Task::none()
            }
            Message::Select(_) | Message::Delete(_) | Message::Clear if !self.busy => {
                let close = matches!(message, Message::Select(_));
                let request = match message {
                    Message::Select(id) => Request::Select { id },
                    Message::Delete(id) => Request::Delete { id },
                    _ => Request::Clear,
                };
                let root = self.root.clone();
                self.busy = true;
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            cabby_clipboard::client(&root, &request).map(|_| ())
                        })
                        .await
                        .unwrap_or_else(|_| Err("clipboard operation failed".into()))
                    },
                    move |result| Message::Done(close, result),
                )
            }
            Message::Done(close, result) => {
                self.busy = false;
                if let Err(e) = result {
                    self.error = Some(e);
                    Task::none()
                } else if close {
                    return Action::Close;
                } else {
                    self.refresh()
                }
            }
            Message::Close => return Action::Close,
            _ => Task::none(),
        };
        Action::Update(task)
    }
    pub fn view(&self, config: &cabby_config::Config) -> Element<'_, Message> {
        let mut contents = column![
            row![
                crate::ui::heading(config, "Clipboard").width(iced::Length::Fill),
                crate::ui::button("Close")
                    .style(crate::ui::quiet)
                    .on_press(Message::Close)
            ],
            text_input("Search history", &self.query)
                .style(crate::ui::input_style)
                .padding([8, 10])
                .id("clipboard-query")
                .on_input(Message::Query)
                .on_submit(Message::Accept)
        ]
        .spacing(10);
        if let Some(error) = &self.error {
            contents = contents.push(text(error));
        }
        if let Some(state) = &self.state {
            if state.state != "watching" {
                contents = contents.push(text(format!("Clipboard {}", state.state)));
            }
            if state.entries.is_empty() {
                contents = contents.push(text("No matching history"));
            }
            let mut entries = column![].spacing(8);
            for (index, item) in state.entries.iter().enumerate() {
                let label = item.preview.replace('\n', " ↵ ").replace('\t', " ");
                let mut select = crate::ui::button(text(label).size(f32::from(config.font_size)))
                    .width(iced::Length::Fill)
                    .style(if index == self.selected {
                        crate::ui::selected
                    } else {
                        crate::ui::standard
                    });
                let mut delete = crate::ui::button("Delete");
                if !self.busy {
                    select = select.on_press(Message::Select(item.id));
                    delete = delete.on_press(Message::Delete(item.id));
                }
                entries = entries.push(row![select, delete].spacing(6));
            }
            contents = contents.push(scrollable(entries).height(iced::Length::Fill));
        }
        let clear = crate::ui::button("Clear history");
        contents = contents.push(if self.busy {
            clear
        } else {
            clear.on_press(Message::Clear)
        });
        iced::widget::container(contents)
            .padding(16)
            .width(iced::Length::Fill)
            .height(iced::Length::Fill)
            .style(crate::ui::panel(config.renderer))
            .into()
    }
}
#[derive(Default, serde::Deserialize)]
pub(crate) struct Revision {
    #[serde(rename = "revision")]
    pub _revision: u64,
}
