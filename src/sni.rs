use anyhow::Context;
use event_listener::{Event, Listener};
use futures_util::stream::StreamExt;
use std::collections::{BTreeMap, HashMap};
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;
use zbus::{interface, proxy};

use tracing::*;

mod protocol;
mod sni_item;

use protocol::i3bar;

#[derive(Clone)]
pub struct StatusNotifierItemProperties {
    pub category: Option<String>,
    pub id: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub window_id: Option<i32>,
    pub icon_name: Option<String>,
    pub icon_pixmap: Option<Vec<(i32, i32, Vec<u8>)>>,
    pub tool_tip: Option<(String, Vec<(i32, i32, Vec<u8>)>, String, String)>,
    pub item_is_menu: Option<bool>,
    pub menu: Option<zbus::zvariant::OwnedObjectPath>,
    pub icon_theme_path: Option<String>,
}

impl Default for StatusNotifierItemProperties {
    fn default() -> Self {
        Self {
            category: None,
            id: None,
            title: None,
            status: None,
            window_id: None,
            icon_name: None,
            icon_pixmap: None,
            tool_tip: None,
            item_is_menu: None,
            menu: None,
            icon_theme_path: None,
        }
    }
}

impl std::fmt::Debug for StatusNotifierItemProperties {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusNotifierItemProperties")
            .field("category", &self.category)
            .field("id", &self.id)
            .field("title", &self.title)
            .field("status", &self.status)
            .field("window_id", &self.window_id)
            .field("icon_name", &self.icon_name)
            .field(
                "icon_pixmap",
                &self.icon_pixmap.as_ref().map(|v| {
                    v.iter()
                        .map(|(w, h, d)| format!("{}x{} ({} bytes)", w, h, d.len()))
                        .collect::<Vec<_>>()
                }),
            )
            .field(
                "tool_tip",
                &self.tool_tip.as_ref().map(|(name, pixmaps, title, desc)| {
                    (
                        name,
                        pixmaps
                            .iter()
                            .map(|(w, h, d)| format!("{}x{} ({} bytes)", w, h, d.len()))
                            .collect::<Vec<_>>(),
                        title,
                        desc,
                    )
                }),
            )
            .field("item_is_menu", &self.item_is_menu)
            .field("menu", &self.menu)
            .field("icon_theme_path", &self.icon_theme_path)
            .finish()
    }
}

impl StatusNotifierItemProperties {
    fn from_map(map: HashMap<String, zbus::zvariant::OwnedValue>) -> Self {
        let mut props = Self::default();
        for (k, v) in map {
            match k.as_str() {
                "Category" => props.category = v.try_into().ok(),
                "Id" => props.id = v.try_into().ok(),
                "Title" => props.title = v.try_into().ok(),
                "Status" => props.status = v.try_into().ok(),
                "WindowId" => props.window_id = v.try_into().ok(),
                "IconName" => props.icon_name = v.try_into().ok(),
                "IconPixmap" => props.icon_pixmap = v.try_into().ok(),
                "ToolTip" => props.tool_tip = v.try_into().ok(),
                "ItemIsMenu" => props.item_is_menu = v.try_into().ok(),
                "Menu" => props.menu = v.try_into().ok(),
                "IconThemePath" => props.icon_theme_path = v.try_into().ok(),
                _ => {}
            }
        }
        props
    }

    fn to_block(&self) -> i3bar::Block {
        let mut other: BTreeMap<String, serde_json::Value> = BTreeMap::new();

        if let Some(ref category) = self.category {
            other.insert("category".into(), category.clone().into());
        }
        if let Some(ref title) = self.title {
            other.insert("title".into(), title.clone().into());
        }
        if let Some(ref status) = self.status {
            other.insert("status".into(), status.clone().into());
        }
        if let Some(window_id) = self.window_id {
            other.insert("window_id".into(), window_id.into());
        }
        if let Some(ref icon_name) = self.icon_name {
            other.insert("icon_name".into(), icon_name.clone().into());
        }
        if let Some(ref icon_theme_path) = self.icon_theme_path {
            other.insert("icon_theme_path".into(), icon_theme_path.clone().into());
        }

        // Send only the largest pixmap.
        if let Some(ref pixmaps) = self.icon_pixmap {
            if let Some((w, h, data)) = pixmaps.iter().max_by_key(|(w, h, _)| w * h) {
                other.insert("pixmap_width".into(), (*w).into());
                other.insert("pixmap_height".into(), (*h).into());
                let pixel_values: Vec<serde_json::Value> =
                    data.iter().map(|b| (*b).into()).collect();
                other.insert("pixmap".into(), pixel_values.into());
            }
        }

        // Combine tooltip title and description into a single string.
        if let Some(ref tool_tip) = self.tool_tip {
            let (_, _, ref tip_title, ref tip_desc) = tool_tip;
            let tooltip = match (tip_title.is_empty(), tip_desc.is_empty()) {
                (false, false) => format!("{}: {}", tip_title, tip_desc),
                (false, true) => tip_title.clone(),
                (true, false) => tip_desc.clone(),
                (true, true) => String::new(),
            };
            if !tooltip.is_empty() {
                other.insert("tooltip".into(), tooltip.into());
            }
        }

        let full_text = other
            .get("tooltip")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| self.title.clone())
            .or_else(|| self.id.clone())
            .unwrap_or_default();

        i3bar::Block {
            name: Some("sni".into()),
            instance: self.id.clone(),
            full_text,
            other,
        }
    }
}

async fn print_all_items(
    items: &tokio::sync::Mutex<BTreeMap<zbus::names::BusName<'static>, StatusNotifierItemInfo>>,
) {
    let items = items.lock().await;
    let blocks: Vec<i3bar::Block> = items
        .values()
        .map(|info| info.properties.to_block())
        .collect();
    if let Ok(output) = serde_json::to_string(&blocks) {
        println!("{},", output);
    }
}

#[derive(Debug, Clone)]
pub struct StatusNotifierItemInfo {
    pub properties: StatusNotifierItemProperties,
}

const SNI_HOST_NAME: &str = "org.kde.StatusNotifierHost";
const SNI_ITEM_NAME: &str = "org.kde.StatusNotifierItem";
const SNI_WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

struct StatusNotifierHost {}

#[interface(interface = "org.kde.StatusNotifierHost")]
impl StatusNotifierHost {}

struct StatusNotifierWatcher {
    host_name: Option<String>,
    items: std::sync::Arc<
        tokio::sync::Mutex<BTreeMap<zbus::names::BusName<'static>, StatusNotifierItemInfo>>,
    >,
    session: zbus::Connection,
    dbus_proxy: zbus::fdo::DBusProxy<'static>,
}

impl StatusNotifierWatcher {
    async fn stream_updates(
        session: zbus::Connection,
        dbus_proxy: zbus::fdo::DBusProxy<'static>,
        owned_emitter: SignalEmitter<'_>,
        items: std::sync::Arc<
            tokio::sync::Mutex<BTreeMap<zbus::names::BusName<'static>, StatusNotifierItemInfo>>,
        >,
        dest: String,
        path: String,
        service: String,
    ) -> anyhow::Result<()> {
        let proxy = sni_item::StatusNotifierItemProxy::builder(&session)
            .destination(dest.clone())
            .context(format!("Error setting destination for {}", service))?
            .path(path.clone())
            .context(format!("Error setting path for {}", service))?
            .cache_properties(proxy::CacheProperties::No)
            .build()
            .await
            .context(format!("Error building proxy for {}", service))?;

        {
            let mut items = items.lock().await;

            let bus_name = zbus::names::BusName::try_from(dest.as_str())
                .context(format!("Error converting service {} to BusName", dest))?;

            debug!("Successfully registered: {}", bus_name);
            debug!("Inserting item key: {:?}", bus_name);
            items.insert(
                bus_name.to_owned(),
                StatusNotifierItemInfo {
                    properties: StatusNotifierItemProperties::default(),
                },
            );
        }

        owned_emitter
            .status_notifier_item_registered(&service)
            .await
            .context(format!("Error emitting item registered for {}", service))?;

        let props_proxy = zbus::fdo::PropertiesProxy::new(&session, dest.clone(), path.clone())
            .await
            .unwrap();
        let raw_props = props_proxy
            .get_all(InterfaceName::try_from(SNI_ITEM_NAME).unwrap())
            .await
            .unwrap();
        // Convert to OwnedValue
        let props: HashMap<String, zbus::zvariant::OwnedValue> = raw_props
            .into_iter()
            .map(|(k, v)| (k, v.try_to_owned().unwrap()))
            .collect();
        let item_props = StatusNotifierItemProperties::from_map(props);
        debug!("Initial properties {}: {:?}", dest, item_props);

        {
            let mut items = items.lock().await;
            let bus_name_key = zbus::names::BusName::try_from(dest.as_str())
                .unwrap()
                .to_owned();
            if let Some(item) = items.get_mut(&bus_name_key) {
                item.properties = item_props;
            }
        }
        print_all_items(&items).await;

        let inner_proxy = proxy.inner();

        let mut all_signals = inner_proxy.receive_all_signals().await.unwrap();
        let mut owner_changes = dbus_proxy.receive_name_owner_changed().await.unwrap();

        loop {
            tokio::select! {
                Some(_) = all_signals.next() => {
                    let raw_props = props_proxy
                        .get_all(InterfaceName::try_from(SNI_ITEM_NAME).unwrap())
                        .await
                        .unwrap();
                    let props: HashMap<String, zbus::zvariant::OwnedValue> = raw_props
                        .into_iter()
                        .map(|(k, v)| (k, v.try_to_owned().unwrap()))
                        .collect();
                    let item_props = StatusNotifierItemProperties::from_map(props);
                    debug!("Properties updated {}: {:?}", dest, item_props);
                    {
                        let mut items = items.lock().await;
                        let bus_name_key = zbus::names::BusName::try_from(dest.as_str())
                            .unwrap()
                            .to_owned();
                        if let Some(item) = items.get_mut(&bus_name_key) {
                            item.properties = item_props.clone();
                        }
                    }
                    print_all_items(&items).await;
                }
                Some(owner_change) = owner_changes.next() => {
                    let args = owner_change.args()?;
                    if args.name().as_str() == dest {
                        {
                            let mut items = items.lock().await;
                            let name = args.name().to_owned();
                            items.remove(&name);
                            owned_emitter.status_notifier_item_unregistered(&name).await.unwrap();
                        }
                        print_all_items(&items).await;
                        break;
                    }
                }
            }
        }
        debug!("Service {} disconnected", dest);
        Ok(())
    }
}

#[interface(interface = "org.kde.StatusNotifierWatcher")]
impl StatusNotifierWatcher {
    async fn register_status_notifier_host(
        &mut self,
        service: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Registering host: {}", service);
        self.host_name = Some(service.to_string());
        emitter.status_notifier_host_registered().await?;
        Ok(())
    }

    async fn register_status_notifier_item(
        &mut self,
        service: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        debug!("Registering request for: {}", service);

        let (dest, path) = if service.starts_with("/") {
            if let Some(sender) = header.sender() {
                debug!("Sender: {}", sender);
                (sender.to_string(), service.to_string())
            } else {
                (service.to_string(), "/StatusNotifierItem".to_string())
            }
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };

        let items = self.items.clone();
        let session = self.session.clone();
        let service = service.to_string();
        let owned_emitter = emitter.to_owned();
        let dbus_proxy = self.dbus_proxy.clone();

        debug!("Service/destination: {:?}", service);
        debug!("Bus name: {}, Path: {}", dest, path);

        tokio::spawn(async move {
            if let Err(e) = Self::stream_updates(
                session,
                dbus_proxy,
                owned_emitter,
                items,
                dest,
                path.clone(),
                service.clone(),
            )
            .await
            {
                error!("Error streaming updates for {} at {}: {}", service, path, e);
            }
        });

        Ok(())
    }

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_unregistered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        self.host_name.is_some()
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        1
    }

    #[zbus(property)]
    async fn registered_status_notifier_items(&self) -> Vec<String> {
        let items = self.items.lock().await;
        items
            .keys()
            .cloned()
            .map(|name| name.as_str().to_string())
            .collect()
    }
}

#[proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    fn register_status_notifier_host(&self, service: &str) -> zbus::Result<()>;
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_thread_names(true)
        .compact();

    let filter = if cfg!(debug_assertions) {
        EnvFilter::new("none,oatbar_sni=debug")
    } else {
        EnvFilter::new("info")
    };

    let registry = tracing_subscriber::registry().with(stderr_layer.with_filter(filter));
    registry.init();

    println!("{}", serde_json::to_string(&i3bar::Header::default())?);
    println!("[");

    let done = Event::new();
    let listener = done.listen();

    let session = zbus::Connection::session().await?;
    let dbus_proxy = zbus::fdo::DBusProxy::new(&session).await?;

    let host_name = format!("{}-{}", SNI_HOST_NAME, std::process::id());
    let _host = zbus::connection::Builder::session()?
        .name(host_name.clone())?
        .serve_at("/StatusNotifierHost", StatusNotifierHost {})?
        .build()
        .await?;

    let _watcher = zbus::connection::Builder::session()?
        .name(SNI_WATCHER_NAME)?
        .serve_at(
            "/StatusNotifierWatcher",
            StatusNotifierWatcher {
                session,
                dbus_proxy,
                items: std::sync::Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
                host_name: None,
            },
        )?
        .build()
        .await?;

    let connection = zbus::Connection::session().await?;
    let proxy = StatusNotifierWatcherProxy::new(&connection).await?;
    proxy.register_status_notifier_host(&host_name).await?;

    listener.wait();

    Ok(())
}
