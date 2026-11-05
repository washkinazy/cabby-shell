//! One bounded menu level at a time, including lazy submenus.
use super::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub token: Token,
    pub path: String,
    pub parent: i32,
    pub entries: Vec<Entry>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub submenu: bool,
    pub checked: Option<bool>,
}
type Layout = (i32, Properties, Vec<OwnedValue>);
pub async fn load(connection: &Connection, token: &Token, path: &str, parent: i32) -> Result<Menu> {
    let proxy = Proxy::new(
        connection,
        token.owner.as_str(),
        path,
        "com.canonical.dbusmenu",
    )
    .await?;
    // Some exporters omit AboutToShow; GetLayout still provides the static menu.
    let _ = proxy.call::<_, _, bool>("AboutToShow", &(parent,)).await;
    let reply = proxy
        .call_method("GetLayout", &(parent, 1i32, Vec::<String>::new()))
        .await?;
    if reply.body().len() > 256 * 1024 {
        return Err(Error::InvalidData);
    }
    let (_, (_, _, children)): (u32, Layout) = reply.body().deserialize()?;
    if children.len() > 128 {
        return Err(Error::InvalidData);
    }
    let mut entries = Vec::new();
    for child in children {
        let (id, props, _): Layout = child.try_into().map_err(|_| Error::InvalidData)?;
        if !boolean(&props, "visible", true) {
            continue;
        }
        entries.push(Entry {
            id,
            label: string(&props, "label").replace('_', ""),
            enabled: boolean(&props, "enabled", true),
            separator: string(&props, "type") == "separator",
            submenu: string(&props, "children-display") == "submenu",
            checked: (!string(&props, "toggle-type").is_empty()).then(|| {
                props
                    .get("toggle-state")
                    .and_then(|v| i32::try_from(v).ok())
                    == Some(1)
            }),
        });
    }
    Ok(Menu {
        token: token.clone(),
        path: path.into(),
        parent,
        entries,
    })
}
