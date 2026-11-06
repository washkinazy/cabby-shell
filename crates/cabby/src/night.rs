//! Native per-output gamma controls. Ramp/schedule behavior informed by Noctalia;
//! its MIT notice is retained in licenses/noctalia-MIT.txt.
use std::{
    collections::HashMap,
    fs::File,
    io::{Seek, Write},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd},
        unix::net::UnixStream,
    },
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, delegate_noop,
    protocol::{wl_output, wl_registry},
};
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};
#[derive(Clone, Debug, PartialEq)]
struct Settings {
    nightlight: cabby_config::Nightlight,
    location: cabby_config::Location,
}
impl From<&cabby_config::Config> for Settings {
    fn from(c: &cabby_config::Config) -> Self {
        Self {
            nightlight: c.nightlight.clone(),
            location: c.location.clone(),
        }
    }
}
fn target(settings: &Settings, clock: crate::schedule::Calendar) -> Option<u16> {
    let n = &settings.nightlight;
    if n.force {
        return Some(n.temperature_night);
    }
    if !n.enabled {
        return None;
    }
    let e = crate::schedule::evaluate(&settings.location, clock)?;
    let warm = if e.since < 30. {
        if e.night {
            0.5 + e.since / 60.
        } else {
            0.5 - e.since / 60.
        }
    } else if e.until < 30. {
        if e.night {
            0.5 + e.until / 60.
        } else {
            0.5 - e.until / 60.
        }
    } else if e.night {
        1.
    } else {
        0.
    };
    Some(
        (f64::from(n.temperature_day)
            + (f64::from(n.temperature_night) - f64::from(n.temperature_day)) * warm)
            .round() as u16,
    )
}
fn interval(settings: &Settings) -> Duration {
    let n = &settings.nightlight;
    let transitioning = !n.force
        && crate::schedule::Calendar::now()
            .and_then(|c| crate::schedule::evaluate(&settings.location, c))
            .is_some_and(|e| e.since < 30. || e.until < 30.);
    let seconds = if transitioning {
        (180000 / u64::from(n.temperature_day - n.temperature_night)).clamp(2, 60)
    } else {
        60
    };
    Duration::from_secs(seconds)
}
fn now_target(settings: &Settings) -> Option<u16> {
    crate::schedule::Calendar::now().and_then(|c| target(settings, c))
}
fn ramp(size: u32, temperature: u16) -> Option<Vec<u8>> {
    if !(2..=65536).contains(&size) || !(1000..=10000).contains(&temperature) {
        return None;
    }
    let t = f64::from(temperature) / 100.;
    let gains = if t <= 66. {
        [
            1.,
            ((99.4708025861 * t.ln() - 161.1195681661) / 255.).clamp(0., 1.),
            if t <= 19. {
                0.
            } else {
                ((138.5177312231 * (t - 10.).ln() - 305.0447927307) / 255.).clamp(0., 1.)
            },
        ]
    } else {
        [
            (329.698727446 * (t - 60.).powf(-0.1332047592) / 255.).clamp(0., 1.),
            (288.1221695283 * (t - 60.).powf(-0.0755148492) / 255.).clamp(0., 1.),
            1.,
        ]
    };
    let mut bytes = Vec::with_capacity(size as usize * 6);
    for gain in gains {
        for i in 0..size {
            bytes.extend_from_slice(
                &((f64::from(i) * 65535. / f64::from(size - 1) * gain).round() as u16)
                    .to_ne_bytes(),
            );
        }
    }
    Some(bytes)
}
fn upload(control: &ZwlrGammaControlV1, size: u32, temperature: u16) -> std::io::Result<()> {
    let bytes =
        ramp(size, temperature).ok_or_else(|| std::io::Error::other("invalid gamma size"))?;
    let fd = unsafe { libc::memfd_create(c"cabby-gamma".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(&bytes)?;
    // SCM_RIGHTS preserves the open-file offset; niri reads from that cursor.
    file.rewind()?;
    control.set_gamma(file.as_fd());
    Ok(())
}
struct Desired {
    settings: Settings,
    suppressed: std::collections::BTreeSet<String>,
    suppress_all: bool,
    enabled_override: Option<bool>,
    force_override: Option<bool>,
    epoch: u64,
    stop: bool,
}
fn effective(d: &Desired) -> Settings {
    let mut s = d.settings.clone();
    s.nightlight.enabled = d.enabled_override.unwrap_or(s.nightlight.enabled);
    s.nightlight.force = d.force_override.unwrap_or(s.nightlight.force);
    s
}
pub struct Resume {
    desired: Arc<Mutex<Desired>>,
    wake: UnixStream,
}
impl Resume {
    pub fn resume(&self) {
        self.desired.lock().unwrap().epoch += 1;
        let _ = (&self.wake).write(&[1]);
    }
}
pub struct Inhibit {
    desired: Arc<Mutex<Desired>>,
    wake: UnixStream,
}
impl Inhibit {
    pub fn apply(&self, outputs: std::collections::BTreeSet<String>, all: bool) {
        let mut d = self.desired.lock().unwrap();
        if d.suppressed == outputs && d.suppress_all == all {
            return;
        }
        d.suppressed = outputs;
        d.suppress_all = all;
        drop(d);
        let _ = (&self.wake).write(&[1]);
    }
}
pub struct Night {
    desired: Arc<Mutex<Desired>>,
    wake: UnixStream,
    status: Arc<Mutex<serde_json::Value>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Night {
    pub fn new(config: &cabby_config::Config, root: &std::path::Path) -> Self {
        let (wake, reader) = UnixStream::pair().expect("gamma wake socket");
        wake.set_nonblocking(true).unwrap();
        reader.set_nonblocking(true).unwrap();
        let desired = Arc::new(Mutex::new(Desired {
            settings: config.into(),
            suppressed: Default::default(),
            suppress_all: false,
            enabled_override: None,
            force_override: None,
            epoch: 0,
            stop: false,
        }));
        let status = Arc::new(Mutex::new(serde_json::json!({"state":"starting"})));
        let d = desired.clone();
        let s = status.clone();
        let path = root.join("nightlight.json");
        let thread = std::thread::spawn(move || worker(d, s, reader, path));
        Self {
            desired,
            wake,
            status,
            thread: Some(thread),
        }
    }
    pub fn configure(&self, config: &cabby_config::Config) {
        let settings = config.into();
        let mut d = self.desired.lock().unwrap();
        if d.settings == settings {
            return;
        }
        if d.settings.nightlight.enabled != settings.nightlight.enabled {
            d.enabled_override = None;
        }
        if d.settings.nightlight.force != settings.nightlight.force {
            d.force_override = None;
        }
        d.settings = settings;
        drop(d);
        let _ = (&self.wake).write(&[1]);
    }
    pub fn policy_handle(&self) -> Inhibit {
        Inhibit {
            desired: self.desired.clone(),
            wake: self.wake.try_clone().expect("gamma policy wake"),
        }
    }
    pub fn observing(&self) -> bool {
        let s = effective(&self.desired.lock().unwrap());
        s.nightlight.enabled || s.nightlight.force
    }
    pub fn control(&self, command: &str) {
        let mut d = self.desired.lock().unwrap();
        let n = effective(&d).nightlight;
        match command {
            "nightlight-enable" => d.enabled_override = Some(true),
            "nightlight-disable" => {
                d.enabled_override = Some(false);
                d.force_override = Some(false);
            }
            "nightlight-force-toggle" => d.force_override = Some(!n.force),
            "nightlight-toggle" => {
                if n.force {
                    d.force_override = Some(false);
                    d.enabled_override = Some(true);
                } else {
                    d.enabled_override = Some(!n.enabled);
                }
            }
            _ => (),
        };
        drop(d);
        let _ = (&self.wake).write(&[1]);
    }
    pub fn resume_handle(&self) -> Resume {
        Resume {
            desired: self.desired.clone(),
            wake: self.wake.try_clone().unwrap(),
        }
    }
    pub fn status(&self) -> serde_json::Value {
        self.status.lock().unwrap().clone()
    }
}
impl Drop for Night {
    fn drop(&mut self) {
        self.desired.lock().unwrap().stop = true;
        let _ = (&self.wake).write(&[1]);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct Output {
    name: String,
    output: wl_output::WlOutput,
    control: Option<ZwlrGammaControlV1>,
    size: u32,
    failed: bool,
    last: Option<u16>,
}
struct State {
    manager: Option<(u32, ZwlrGammaControlManagerV1)>,
    outputs: HashMap<u32, Output>,
    temperature: Option<u16>,
    suppressed: std::collections::BTreeSet<String>,
    suppress_all: bool,
}
impl State {
    fn apply(&mut self, qh: &QueueHandle<Self>) {
        for (id, out) in &mut self.outputs {
            let Some(temperature) = self
                .temperature
                .filter(|_| !self.suppress_all && !self.suppressed.contains(&out.name))
            else {
                if let Some(c) = out.control.take() {
                    c.destroy();
                }
                out.size = 0;
                out.last = None;
                continue;
            };
            if out.failed {
                continue;
            }
            if out.control.is_none()
                && let Some((_, manager)) = &self.manager
            {
                out.control = Some(manager.get_gamma_control(&out.output, qh, *id));
            }
            if out.size > 0 && out.last != Some(temperature) {
                if upload(out.control.as_ref().unwrap(), out.size, temperature).is_err() {
                    out.failed = true;
                    if let Some(c) = out.control.take() {
                        c.destroy();
                    }
                } else {
                    out.last = Some(temperature);
                }
            }
        }
    }
    fn release(&mut self) {
        for out in self.outputs.values_mut() {
            if let Some(c) = out.control.take() {
                c.destroy();
            }
            out.size = 0;
            out.last = None;
        }
    }
    fn status(&self) -> serde_json::Value {
        let failed = self.outputs.values().filter(|o| o.failed).count();
        let applied = self
            .outputs
            .values()
            .filter(|o| o.control.is_some() && !o.failed && o.last.is_some())
            .count();
        serde_json::json!({"state":if self.manager.is_none(){"unsupported"}else if failed>0{"output-failed"}else if self.temperature.is_none(){"no-schedule"}else if applied>0{"active"}else{"waiting"},"inhibited_outputs":self.outputs.values().filter(|out|self.suppress_all||self.suppressed.contains(&out.name)).count(),"temperature":self.temperature,"outputs":self.outputs.len(),"applied":applied,"failed":failed})
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
            } => {
                match interface.as_str() {
                    "zwlr_gamma_control_manager_v1" if s.manager.is_none() => {
                        s.manager = Some((name, r.bind(name, 1, qh, ())))
                    }
                    "wl_output" if s.outputs.len() < 16 => {
                        s.outputs.insert(
                            name,
                            Output {
                                output: r.bind(name, version.min(4), qh, name),
                                name: String::new(),
                                control: None,
                                size: 0,
                                failed: false,
                                last: None,
                            },
                        );
                    }
                    _ => return,
                }
                s.apply(qh);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if s.manager.as_ref().is_some_and(|(id, _)| *id == name) {
                    s.release();
                    s.manager.take().unwrap().1.destroy();
                }
                if let Some(mut out) = s.outputs.remove(&name) {
                    if let Some(c) = out.control.take() {
                        c.destroy();
                    }
                    if out.output.version() >= 3 {
                        out.output.release();
                    }
                }
            }
            _ => (),
        }
    }
}
impl Dispatch<ZwlrGammaControlV1, u32> for State {
    fn event(
        s: &mut Self,
        c: &ZwlrGammaControlV1,
        e: zwlr_gamma_control_v1::Event,
        id: &u32,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let Some(out) = s.outputs.get_mut(id) else {
            return;
        };
        if out.control.as_ref() != Some(c) {
            return;
        }
        match e {
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                if (2..=65536).contains(&size) {
                    out.size = size;
                } else {
                    out.failed = true;
                    out.control.take().unwrap().destroy();
                }
            }
            zwlr_gamma_control_v1::Event::Failed => {
                out.failed = true;
                out.control.take().unwrap().destroy();
            }
            _ => (),
        };
        s.apply(qh);
    }
}
impl Dispatch<wl_output::WlOutput, u32> for State {
    fn event(
        s: &mut Self,
        _: &wl_output::WlOutput,
        e: wl_output::Event,
        id: &u32,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = e
            && let Some(out) = s.outputs.get_mut(id)
        {
            out.name = name;
            s.apply(qh);
        }
    }
}
delegate_noop!(State: ignore ZwlrGammaControlManagerV1);
struct LocationCache {
    source: Option<cabby_config::Location>,
    coordinates: Option<(f64, f64)>,
    due: Instant,
    epoch: u64,
}
impl LocationCache {
    fn new() -> Self {
        Self {
            source: None,
            coordinates: None,
            due: Instant::now(),
            epoch: 0,
        }
    }
    fn configure(&mut self, location: &cabby_config::Location, epoch: u64) {
        if self.source.as_ref() != Some(location) {
            self.source = Some(location.clone());
            self.coordinates = None;
            self.due = Instant::now();
        }
        if self.epoch != epoch {
            self.due = Instant::now();
            self.epoch = epoch;
        }
    }
}
fn worker(
    desired: Arc<Mutex<Desired>>,
    status: Arc<Mutex<serde_json::Value>>,
    wake: UnixStream,
    path: std::path::PathBuf,
) {
    let mut cache = LocationCache::new();
    loop {
        let (settings, epoch) = {
            let d = desired.lock().unwrap();
            if d.stop {
                return;
            }
            (effective(&d), d.epoch)
        };
        cache.configure(&settings.location, epoch);
        if !settings.nightlight.enabled && !settings.nightlight.force {
            publish(
                &status,
                &path,
                decorate(serde_json::json!({"state":"disabled"}), &settings),
            );
            let _ = crate::idle::wait(&wake, -1, -1);
            continue;
        }
        if let Err(error) = connected(
            &desired, &status, &wake, &settings, epoch, &path, &mut cache,
        ) {
            eprintln!("night light backend unavailable: {error}");
            publish(
                &status,
                &path,
                decorate(serde_json::json!({"state":"unavailable"}), &settings),
            );
            let _ = crate::idle::wait(&wake, -1, 2000);
        }
    }
}
fn connected(
    desired: &Mutex<Desired>,
    status: &Mutex<serde_json::Value>,
    wake: &UnixStream,
    settings: &Settings,
    epoch: u64,
    path: &std::path::Path,
    cache: &mut LocationCache,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    conn.display().get_registry(&qh, ());
    let mut scene = settings.clone();
    if let Some((lat, lon)) = cache.coordinates {
        scene.location.latitude = Some(lat);
        scene.location.longitude = Some(lon);
    }
    let mut state = State {
        manager: None,
        outputs: HashMap::new(),
        temperature: now_target(&scene),
        suppressed: Default::default(),
        suppress_all: false,
    };
    let mut fetch: Option<crate::location::Fetch> = None;
    let network = !settings.location.custom_schedule
        && (settings.location.auto_locate || !settings.location.address.is_empty());
    let mut tick = Instant::now();
    loop {
        {
            let d = desired.lock().unwrap();
            if state.suppressed != d.suppressed || state.suppress_all != d.suppress_all {
                state.suppressed = d.suppressed.clone();
                state.suppress_all = d.suppress_all;
                state.temperature = now_target(&scene);
                state.apply(&qh);
            }
            if d.stop || effective(&d) != *settings || d.epoch != epoch {
                state.release();
                conn.flush()?;
                return Ok(());
            }
        }
        if network && fetch.is_none() && Instant::now() >= cache.due {
            match crate::location::Fetch::start(&settings.location) {
                Ok(value) => fetch = Some(value),
                Err(_) => cache.due = Instant::now() + Duration::from_secs(300),
            }
        }
        if let Some(result) = fetch.as_mut().and_then(crate::location::Fetch::next) {
            fetch = None;
            if let Ok(coordinates) = result {
                cache.coordinates = Some(coordinates);
                cache.due = Instant::now()
                    + if settings.location.auto_locate {
                        Duration::from_secs(21600)
                    } else {
                        Duration::from_secs(2592000)
                    };
            } else {
                cache.due = Instant::now() + Duration::from_secs(300);
            }
            scene = settings.clone();
            if let Some((lat, lon)) = cache.coordinates {
                scene.location.latitude = Some(lat);
                scene.location.longitude = Some(lon);
            }
            state.temperature = now_target(&scene);
            state.apply(&qh);
        }
        queue.dispatch_pending(&mut state)?;
        if tick.elapsed() >= interval(&scene) {
            state.temperature = now_target(&scene);
            state.apply(&qh);
            tick = Instant::now();
        }
        publish(status, path, decorate(state.status(), &scene));
        conn.flush()?;
        let Some(reader) = queue.prepare_read() else {
            continue;
        };
        let timeout = (interval(&scene)
            .saturating_sub(tick.elapsed())
            .as_millis()
            .min(60000)) as i32;
        let timeout = if fetch.is_some() {
            timeout.min(100)
        } else {
            timeout
        };
        if crate::idle::wait(wake, conn.as_fd().as_raw_fd(), timeout)? {
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

fn decorate(mut value: serde_json::Value, settings: &Settings) -> serde_json::Value {
    let n = &settings.nightlight;
    let phase = if n.force {
        "forced"
    } else if !n.enabled {
        "off"
    } else {
        match crate::schedule::Calendar::now()
            .and_then(|c| crate::schedule::evaluate(&settings.location, c))
        {
            Some(e) if e.night => "night",
            Some(_) => "day",
            None => "no-schedule",
        }
    };
    value["enabled"] = n.enabled.into();
    value["force"] = n.force.into();
    value["phase"] = phase.into();
    value
}
fn publish(status: &Mutex<serde_json::Value>, path: &std::path::Path, value: serde_json::Value) {
    use std::os::unix::fs::OpenOptionsExt;
    let mut old = status.lock().unwrap();
    if *old == value {
        return;
    }
    let temporary = path.with_extension("tmp");
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(value.to_string().as_bytes())?;
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        eprintln!("night light runtime snapshot unavailable");
    }
    *old = value;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        let mut c = cabby_config::Config::default();
        c.nightlight.enabled = true;
        c.location.custom_schedule = true;
        c.location.sunset = "20:00".into();
        c.location.sunrise = "07:00".into();
        (&c).into()
    }
    fn clock(minute: f64) -> crate::schedule::Calendar {
        crate::schedule::Calendar {
            minute,
            day: 100,
            offset: 0.,
        }
    }
    #[test]
    fn wall_clock_schedule_fades_startup_force_and_missing_location() {
        let mut s = settings();
        for (minute, k) in [
            (19. * 60. + 30., 6500),
            (20. * 60., 5250),
            (20. * 60. + 30., 4000),
            (6. * 60. + 30., 4000),
            (7. * 60., 5250),
            (7. * 60. + 30., 6500),
            (12. * 60., 6500),
        ] {
            assert_eq!(target(&s, clock(minute)), Some(k));
        }
        s.location.custom_schedule = false;
        assert_eq!(target(&s, clock(0.)), None);
        s.nightlight.enabled = false;
        s.nightlight.force = true;
        assert_eq!(target(&s, clock(0.)), Some(4000));
        s.nightlight.force = false;
        assert_eq!(target(&s, clock(0.)), None);
    }
    #[test]
    fn bounded_native_endian_ramps_have_monotonic_channels() {
        for size in [2, 256, 65536] {
            for temperature in [1000, 4000, 6500, 10000] {
                let bytes = ramp(size, temperature).unwrap();
                assert_eq!(bytes.len(), size as usize * 6);
                let values: Vec<_> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_ne_bytes([b[0], b[1]]))
                    .collect();
                for channel in values.chunks_exact(size as usize) {
                    assert_eq!(channel[0], 0);
                    assert!(channel.windows(2).all(|v| v[0] <= v[1]));
                }
                if temperature == 4000 {
                    let n = size as usize;
                    assert!(
                        values[n - 1] > values[2 * n - 1] && values[2 * n - 1] > values[3 * n - 1]
                    );
                }
            }
        }
        for size in [0, 1, 65537, u32::MAX] {
            assert!(ramp(size, 4000).is_none());
        }
        assert!(ramp(256, 0).is_none());
    }
    #[test]
    fn toggles_from_forced_to_scheduled_then_off_and_config_resets_overrides() {
        let mut c = cabby_config::Config::default();
        let (wake, _reader) = UnixStream::pair().unwrap();
        let night = Night {
            desired: Arc::new(Mutex::new(Desired {
                settings: (&c).into(),
                suppressed: Default::default(),
                suppress_all: false,
                enabled_override: None,
                force_override: None,
                epoch: 0,
                stop: false,
            })),
            wake,
            status: Arc::new(Mutex::new(serde_json::json!({}))),
            thread: None,
        };
        night.control("nightlight-force-toggle");
        assert!(effective(&night.desired.lock().unwrap()).nightlight.force);
        night.control("nightlight-toggle");
        let n = effective(&night.desired.lock().unwrap()).nightlight;
        assert!(n.enabled && !n.force);
        night.control("nightlight-toggle");
        assert!(!effective(&night.desired.lock().unwrap()).nightlight.enabled);
        c.nightlight.enabled = true;
        night.configure(&c);
        assert!(effective(&night.desired.lock().unwrap()).nightlight.enabled);
    }
}
