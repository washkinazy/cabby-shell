//! Only authenticated success may use Iced's normal (unlocking) exit path.
use super::Context;
use crate::{
    plugins::lock::{self, Lock},
    ui,
};
use cabby_lock::{
    auth::Client,
    policy::{Effect, Event, State},
};
use iced::{Element, Subscription, Task};
use iced_exwlshell::{
    sessionlock::application,
    settings::Settings,
    shell::{self, ShellEvent, ShellReceiver},
    to_sessionlock_message,
};
use std::{
    os::{
        linux::net::SocketAddrExt,
        unix::{
            ffi::OsStrExt,
            net::{SocketAddr, UnixDatagram},
        },
    },
    time::Duration,
};

struct Host {
    context: Context,
    lock: Lock,
    shell: ShellReceiver,
    probe: Option<String>,
}
#[to_sessionlock_message]
#[derive(Clone, Debug)]
enum Message {
    Lock(lock::Message),
    Shell(ShellEvent),
    Deadline,
    Probe(u8),
}
fn delay(message: Message, seconds: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_secs(seconds)).await;
            message
        },
        |message| message,
    )
}
fn ready() -> std::io::Result<()> {
    if let Some(path) = std::env::var_os("NOTIFY_SOCKET") {
        let bytes = path.as_bytes();
        let address = if bytes.first() == Some(&b'@') {
            SocketAddr::from_abstract_name(&bytes[1..])
        } else {
            SocketAddr::from_pathname(path)
        }?;
        UnixDatagram::unbound()?.send_to_addr(b"READY=1", &address)?;
    }
    Ok(())
}
impl Host {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Shell(ShellEvent::Locked) => {
                if self.lock.confirmed() == Effect::Ready {
                    if ready().is_err() {
                        std::process::exit(1);
                    }
                    if self.probe.as_deref() == Some("authenticate") {
                        return delay(Message::Probe(0), 2);
                    }
                    if self.probe.as_deref() == Some("crash") {
                        return delay(Message::Probe(2), 2);
                    }
                }
            }
            Message::Shell(ShellEvent::LockDenied) => {
                self.lock.policy.update(Event::Denied);
                std::process::exit(73);
            }
            // The trusted compositor ended ownership (e.g. alternative secure
            // authentication). Upstream already acknowledges `finished` per
            // the protocol. Do not request another unlock or start a new lock.
            Message::Shell(ShellEvent::LockedFinished) => std::process::exit(0),
            Message::Deadline if self.lock.policy.state == State::Acquiring => {
                std::process::exit(1)
            }
            Message::Lock(message) => {
                let rejected = matches!(message, lock::Message::Authentication(_, Err(_)));
                let (effect, task) = self.lock.update(message);
                if effect == Effect::Unlock {
                    return iced::exit();
                }
                if rejected && self.probe.as_deref() == Some("authenticate") {
                    eprintln!("lock-probe: rejection preserved lock");
                    return delay(Message::Probe(1), 2);
                }
                return task.map(Message::Lock);
            }
            Message::Probe(step) if self.probe.is_some() => {
                if step == 2 {
                    std::process::exit(1);
                }
                self.lock
                    .fixture_input(if step == 0 { "wrong" } else { "fixture" });
                return self.update(Message::Lock(lock::Message::Submit));
            }
            _ => {}
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        self.lock.view().map(Message::Lock)
    }
    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.shell.listen().map(Message::Shell),
            iced::event::listen_with(|event, _, _| {
                if let iced::Event::Keyboard(event) = event {
                    Some(Message::Lock(lock::Message::Key(event)))
                } else {
                    None
                }
            }),
        ])
    }
}
pub(super) fn run(context: Context, fixture: Option<(Client, String)>) -> iced_exwlshell::Result {
    // Wayland traces include physical key codes. Never inherit protocol logging
    // into the production password input path; synthetic probes retain traces.
    if fixture.is_none() && std::env::var_os("WAYLAND_DEBUG").is_some() {
        use std::os::unix::process::CommandExt;
        let _ = std::process::Command::new(
            std::env::current_exe().unwrap_or_else(|_| std::process::exit(1)),
        )
        .args(std::env::args_os().skip(1))
        .env_remove("WAYLAND_DEBUG")
        .exec();
        std::process::exit(1);
    }
    cabby_lock::disable_dumps().unwrap_or_else(|_| std::process::exit(1));
    let (client, probe) = if let Some((client, mode)) = fixture {
        (client, Some(mode))
    } else {
        cabby_lock::preflight().unwrap_or_else(|_| {
            eprintln!("Lock authentication policy unavailable; see docs/locking.md");
            std::process::exit(1)
        });
        let helper = std::env::current_exe()
            .unwrap_or_else(|_| std::process::exit(1))
            .with_file_name("cabby-auth");
        use std::os::unix::fs::PermissionsExt;
        if !std::fs::metadata(&helper)
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        {
            eprintln!("cabby-auth helper unavailable");
            std::process::exit(1);
        }
        (Client::new(helper), None)
    };
    let (sender, receiver) = shell::channel();
    let config = context.config.clone();
    application(
        move || {
            (
                Host {
                    context: Context {
                        config: context.config.clone(),
                        source: context.source.clone(),
                    },
                    lock: Lock::new(client.clone()),
                    shell: receiver.clone(),
                    probe: probe.clone(),
                },
                delay(Message::Deadline, 12),
            )
        },
        Host::update,
        Host::view,
    )
    .subscription(Host::subscription)
    .theme(|host: &Host| ui::theme(&host.context.config))
    .settings(Settings {
        shell_broadcast: sender,
        default_text_size: iced::Pixels(f32::from(config.font_size)),
        ..Default::default()
    })
    .run()
}
