#![allow(dead_code)]
use cabby_tray::Properties;
use std::sync::{Arc, Mutex};
use zbus::{
    Connection,
    connection::Builder,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};
#[derive(Default)]
pub struct Data {
    pub activates: usize,
    pub secondary: usize,
    pub status: String,
    pub icon: Option<String>,
    pub clicked: Vec<i32>,
    pub activate_unsupported: bool,
    pub activate_stalled: bool,
    pub about_stalled: bool,
    pub about_changes: bool,
    pub prepared: bool,
    pub require_open: bool,
    pub fail_layout: bool,
    pub about_calls: Vec<i32>,
    pub layout_calls: Vec<i32>,
    pub events: Vec<(i32, String)>,
}
struct Item(Arc<Mutex<Data>>);
#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    async fn activate(&self, _x: i32, _y: i32) -> zbus::fdo::Result<()> {
        let (unsupported, stalled) = {
            let mut data = self.0.lock().unwrap();
            data.activates += 1;
            (data.activate_unsupported, data.activate_stalled)
        };
        if stalled {
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        }
        if unsupported {
            return Err(zbus::fdo::Error::NotSupported("Use exported menu".into()));
        }
        Ok(())
    }
    fn secondary_activate(&self, _x: i32, _y: i32) {
        self.0.lock().unwrap().secondary += 1;
    }
    #[zbus(property)]
    fn title(&self) -> &str {
        "Fixture"
    }
    #[zbus(property)]
    fn id(&self) -> &str {
        "fixture"
    }
    #[zbus(property)]
    fn status(&self) -> String {
        self.0.lock().unwrap().status.clone()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        self.0
            .lock()
            .unwrap()
            .icon
            .clone()
            .unwrap_or_else(|| "fixture-icon".into())
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        if self.0.lock().unwrap().icon.is_some() {
            return Vec::new();
        }
        vec![
            (-1, 2, vec![]),
            (2, 1, vec![128, 250, 30, 20, 255, 40, 50, 60]),
        ]
    }
    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        "/Menu".try_into().unwrap()
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
}
struct Menu(Arc<Mutex<Data>>);
type Layout = (u32, (i32, Properties, Vec<OwnedValue>));
fn props(label: &str) -> Properties {
    Properties::from([("label".into(), Value::from(label).try_to_owned().unwrap())])
}
fn child(id: i32, properties: Properties) -> OwnedValue {
    Value::from((id, properties, Vec::<OwnedValue>::new()))
        .try_into()
        .unwrap()
}
#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    async fn about_to_show(&self, id: i32) -> bool {
        let stalled = {
            let mut data = self.0.lock().unwrap();
            data.about_calls.push(id);
            data.about_stalled
        };
        if stalled {
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        }
        let mut data = self.0.lock().unwrap();
        data.prepared = true;
        data.about_changes
    }
    fn get_layout(
        &self,
        parent: i32,
        depth: i32,
        _properties: Vec<String>,
    ) -> zbus::fdo::Result<Layout> {
        assert_eq!(depth, 1);
        {
            let mut data = self.0.lock().unwrap();
            data.layout_calls.push(parent);
            if data.fail_layout {
                return Err(zbus::fdo::Error::Failed(
                    "Fixture layout unavailable".into(),
                ));
            }
            if data.about_changes && !data.prepared {
                return Ok((1, (parent, Properties::new(), Vec::new())));
            }
            if data.require_open {
                let mut opened = std::collections::HashSet::new();
                for (id, event) in &data.events {
                    match event.as_str() {
                        "opened" => {
                            opened.insert(*id);
                        }
                        "closed" => {
                            opened.remove(id);
                        }
                        _ => (),
                    }
                }
                if !opened.contains(&0) || !opened.contains(&parent) {
                    return Err(zbus::fdo::Error::Failed("Menu was not opened".into()));
                }
            }
        }
        let children = if parent == 0 {
            let mut disabled = props("Disabled");
            disabled.insert("enabled".into(), false.into());
            let mut submenu = props("Submenu");
            submenu.insert(
                "children-display".into(),
                Value::from("submenu").try_to_owned().unwrap(),
            );
            let mut checked = props("_Checked");
            checked.insert(
                "toggle-type".into(),
                Value::from("checkmark").try_to_owned().unwrap(),
            );
            checked.insert("toggle-state".into(), 1i32.into());
            vec![child(1, checked), child(2, disabled), child(3, submenu)]
        } else {
            vec![child(4, props("Subitem"))]
        };
        Ok((1, (parent, Properties::new(), children)))
    }
    fn event(&self, id: i32, event: &str, _data: OwnedValue, _timestamp: u32) {
        let mut data = self.0.lock().unwrap();
        data.events.push((id, event.into()));
        if event == "clicked" {
            data.clicked.push(id);
        }
    }
}
struct Fixture(Arc<Mutex<Data>>);
#[zbus::interface(name = "org.cabby.TrayFixture")]
impl Fixture {
    fn status(&self) -> (u32, u32, u32) {
        let data = self.0.lock().unwrap();
        (
            data.activates as u32,
            data.events.iter().filter(|(_, e)| e == "opened").count() as u32,
            data.events.iter().filter(|(_, e)| e == "closed").count() as u32,
        )
    }
}
pub async fn fixture(address: &str) -> (Connection, Arc<Mutex<Data>>) {
    let data = Arc::new(Mutex::new(Data {
        status: "Active".into(),
        ..Data::default()
    }));
    let c = Builder::address(address)
        .unwrap()
        .serve_at("/StatusNotifierItem", Item(data.clone()))
        .unwrap()
        .serve_at("/Menu", Menu(data.clone()))
        .unwrap()
        .serve_at("/Fixture", Fixture(data.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();
    (c, data)
}

pub async fn export_item(connection: &Connection) {
    let data = Arc::new(Mutex::new(Data {
        status: "Active".into(),
        ..Default::default()
    }));
    connection
        .object_server()
        .at("/StatusNotifierItem", Item(data))
        .await
        .unwrap();
}

pub async fn unready(address: &str) -> Connection {
    // Start the object dispatcher so an unexported item fails immediately,
    // rather than leaving its first call pending until the fixture is exported.
    Builder::address(address)
        .unwrap()
        .serve_at("/Menu", Menu(Arc::new(Mutex::new(Data::default()))))
        .unwrap()
        .build()
        .await
        .unwrap()
}
