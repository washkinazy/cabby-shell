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
    pub clicked: Vec<i32>,
}
struct Item(Arc<Mutex<Data>>);
#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    fn activate(&self, _x: i32, _y: i32) {
        self.0.lock().unwrap().activates += 1;
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
    fn icon_name(&self) -> &str {
        "fixture-icon"
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
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
    fn about_to_show(&self, _id: i32) -> bool {
        false
    }
    fn get_layout(
        &self,
        parent: i32,
        depth: i32,
        _properties: Vec<String>,
    ) -> (u32, (i32, Properties, Vec<OwnedValue>)) {
        assert_eq!(depth, 1);
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
        (1, (parent, Properties::new(), children))
    }
    fn event(&self, id: i32, event: &str, _data: OwnedValue, _timestamp: u32) {
        assert_eq!(event, "clicked");
        self.0.lock().unwrap().clicked.push(id);
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
        .build()
        .await
        .unwrap();
    (c, data)
}
