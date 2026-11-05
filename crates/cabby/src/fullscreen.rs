//! Actual fullscreen/focus/output membership from the foreign-toplevel protocol, never geometry.
use std::{
    collections::{BTreeSet, HashMap},
    os::{
        fd::{AsFd, AsRawFd},
        unix::net::UnixStream,
    },
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    protocol::{wl_output, wl_registry},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};
#[derive(Default, Clone)]
struct Window {
    full: bool,
    minimized: bool,
    focused: bool,
    outputs: BTreeSet<u32>,
}
struct Top {
    pending: Window,
    current: Window,
    handle: ZwlrForeignToplevelHandleV1,
}
struct State {
    h: crate::caffeine::Handle,
    manager: Option<(u32, ZwlrForeignToplevelManagerV1)>,
    outputs: HashMap<wayland_client::backend::ObjectId, (u32, wl_output::WlOutput, String)>,
    tops: HashMap<wayland_client::backend::ObjectId, Top>,
    failed: bool,
}
impl State {
    fn publish(&self) {
        let mut outputs = BTreeSet::new();
        let mut unknown = false;
        let requires_focus = self.h.config().is_none_or(|c| c.fullscreen_requires_focus);
        for top in self.tops.values().filter(|t| {
            t.current.full && !t.current.minimized && (!requires_focus || t.current.focused)
        }) {
            for id in &top.current.outputs {
                if let Some((_, _, name)) = self.outputs.values().find(|(n, _, _)| n == id) {
                    if name.is_empty() {
                        unknown = true;
                    } else {
                        outputs.insert(name.clone());
                    }
                }
            }
        }
        self.h.fullscreen(
            outputs,
            unknown,
            if self.manager.is_some() {
                "watching"
            } else {
                "unsupported"
            },
        );
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        s: &mut Self,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match e {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "zwlr_foreign_toplevel_manager_v1" if s.manager.is_none() && version >= 2 => {
                    s.manager = Some((name, r.bind(name, version.min(3), qh, ())))
                }
                "wl_output" if s.outputs.len() < 32 => {
                    let out: wl_output::WlOutput = r.bind(name, version.min(4), qh, ());
                    s.outputs.insert(out.id(), (name, out, String::new()));
                }
                _ => (),
            },
            wl_registry::Event::GlobalRemove { name } => {
                if s.manager.as_ref().is_some_and(|(id, _)| *id == name) {
                    s.failed = true;
                }
                s.outputs.retain(|_, (id, out, _)| {
                    if *id == name {
                        if out.version() >= 3 {
                            out.release();
                        }
                        false
                    } else {
                        true
                    }
                });
                for top in s.tops.values_mut() {
                    top.pending.outputs.remove(&name);
                    top.current.outputs.remove(&name);
                }
            }
            _ => (),
        }
        s.publish();
    }
}
impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        e: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                if s.tops.len() >= 512 {
                    s.failed = true;
                    return;
                }
                s.tops.insert(
                    toplevel.id(),
                    Top {
                        pending: Window::default(),
                        current: Window::default(),
                        handle: toplevel,
                    },
                );
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => s.failed = true,
            _ => (),
        }
    }
    wayland_client::event_created_child!(State,ZwlrForeignToplevelManagerV1,[0 => (ZwlrForeignToplevelHandleV1,())]);
}
impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        s: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        e: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_handle_v1::Event::Closed = e {
            if let Some(top) = s.tops.remove(&handle.id()) {
                top.handle.destroy();
            }
            s.publish();
            return;
        }
        let Some(top) = s.tops.get_mut(&handle.id()) else {
            return;
        };
        match e {
            zwlr_foreign_toplevel_handle_v1::Event::State { state } => {
                if state.len() > 128 || state.len() % 4 != 0 {
                    s.failed = true;
                    return;
                }
                let flags: Vec<_> = state
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_ne_bytes(*b))
                    .collect();
                top.pending.full = flags.contains(&3);
                top.pending.minimized = flags.contains(&1);
                top.pending.focused = flags.contains(&2);
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { output } => {
                if let Some((id, _, _)) = s.outputs.get(&output.id()) {
                    top.pending.outputs.insert(*id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { output } => {
                if let Some((id, _, _)) = s.outputs.get(&output.id()) {
                    top.pending.outputs.remove(id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                top.current = top.pending.clone();
                s.publish();
            }
            _ => (),
        }
    }
}
impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        s: &mut Self,
        out: &wl_output::WlOutput,
        e: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = e
            && let Some((_, _, value)) = s.outputs.get_mut(&out.id())
        {
            *value = if name.len() <= 256 && !name.chars().any(char::is_control) {
                name
            } else {
                String::new()
            };
            s.publish();
        }
    }
}
pub(super) fn run(h: crate::caffeine::Handle, wake: UnixStream) {
    loop {
        let Some(config) = h.config() else {
            return;
        };
        if !config.fullscreen {
            h.fullscreen(BTreeSet::new(), false, "disabled");
            let _ = crate::idle::wait(&wake, -1, -1);
            continue;
        }
        if connected(&h, &wake).is_err() {
            h.fullscreen(BTreeSet::new(), false, "unavailable");
            let _ = crate::idle::wait(&wake, -1, 2000);
        }
    }
}
fn connected(
    h: &crate::caffeine::Handle,
    wake: &UnixStream,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::connect_to_env()?;
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    connection.display().get_registry(&qh, ());
    let mut state = State {
        h: h.clone(),
        manager: None,
        outputs: HashMap::new(),
        tops: HashMap::new(),
        failed: false,
    };
    loop {
        if !h.config().is_some_and(|c| c.fullscreen) {
            return Ok(());
        }
        queue.dispatch_pending(&mut state)?;
        state.publish();
        if state.failed {
            return Err("fullscreen observation lost".into());
        }
        connection.flush()?;
        let Some(reader) = queue.prepare_read() else {
            continue;
        };
        if crate::idle::wait(wake, connection.as_fd().as_raw_fd(), -1)? {
            match reader.read() {
                Ok(_) => (),
                Err(wayland_client::backend::WaylandError::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}
