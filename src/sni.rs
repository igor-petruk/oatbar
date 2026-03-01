use anyhow::Context;
use clap::{Args, Parser, Subcommand, ValueEnum};
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

type ItemMap = std::sync::Arc<
    tokio::sync::Mutex<BTreeMap<zbus::names::BusName<'static>, StatusNotifierItemProperties>>,
>;

#[derive(Clone)]
pub struct Pixmap {
    pub width: i32,
    pub height: i32,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Pixmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Pixmap({}x{}, {} bytes)",
            self.width,
            self.height,
            self.data.len()
        )
    }
}

impl From<(i32, i32, Vec<u8>)> for Pixmap {
    fn from((width, height, data): (i32, i32, Vec<u8>)) -> Self {
        Self {
            width,
            height,
            data,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tooltip {
    pub icon_name: String,
    pub icon_pixmaps: Vec<Pixmap>,
    pub title: String,
    pub description: String,
}

impl Tooltip {
    fn display_text(&self) -> Option<String> {
        match (self.title.is_empty(), self.description.is_empty()) {
            (false, false) => Some(format!("{}: {}", self.title, self.description)),
            (false, true) => Some(self.title.clone()),
            (true, false) => Some(self.description.clone()),
            (true, true) => None,
        }
    }
}

impl From<(String, Vec<(i32, i32, Vec<u8>)>, String, String)> for Tooltip {
    fn from(
        (icon_name, pixmaps, title, description): (
            String,
            Vec<(i32, i32, Vec<u8>)>,
            String,
            String,
        ),
    ) -> Self {
        Self {
            icon_name,
            icon_pixmaps: pixmaps.into_iter().map(Pixmap::from).collect(),
            title,
            description,
        }
    }
}

#[derive(Clone, Default, Debug)]
pub struct StatusNotifierItemProperties {
    pub dbus_destination: String,
    pub visible: bool,
    pub category: Option<String>,
    pub id: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub window_id: Option<i32>,
    pub icon_name: Option<String>,
    pub icon_pixmap: Option<Vec<Pixmap>>,
    pub tool_tip: Option<Tooltip>,
    pub item_is_menu: Option<bool>,
    pub menu: Option<zbus::zvariant::OwnedObjectPath>,
    pub icon_theme_path: Option<String>,
}

impl StatusNotifierItemProperties {
    fn from_map(
        dbus_destination: String,
        map: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> Self {
        let mut props = Self {
            dbus_destination,
            visible: true,
            ..Default::default()
        };
        for (k, v) in map {
            match k.as_str() {
                "Category" => props.category = v.try_into().ok(),
                "Id" => props.id = v.try_into().ok(),
                "Title" => props.title = v.try_into().ok(),
                "Status" => props.status = v.try_into().ok(),
                "WindowId" => props.window_id = v.try_into().ok(),
                "IconName" => props.icon_name = v.try_into().ok(),
                "IconPixmap" => {
                    let raw: Option<Vec<(i32, i32, Vec<u8>)>> = v.try_into().ok();
                    props.icon_pixmap = raw.map(|v| v.into_iter().map(Pixmap::from).collect());
                }
                "ToolTip" => {
                    let raw: Option<(String, Vec<(i32, i32, Vec<u8>)>, String, String)> =
                        v.try_into().ok();
                    props.tool_tip = raw.map(Tooltip::from);
                }
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

        let visible_str = if self.visible { "1" } else { "" };
        other.insert("visible".into(), visible_str.into());
        other.insert("dbus".into(), self.dbus_destination.clone().into());

        for (key, val) in [
            ("category", &self.category),
            ("title", &self.title),
            ("status", &self.status),
            ("icon_name", &self.icon_name),
            ("icon_theme_path", &self.icon_theme_path),
        ] {
            if let Some(v) = val {
                other.insert(key.into(), v.clone().into());
            }
        }

        if let Some(window_id) = self.window_id {
            other.insert("window_id".into(), window_id.into());
        }

        if let Some(ref pixmaps) = self.icon_pixmap {
            if let Some(pixmap) = pixmaps.iter().max_by_key(|p| p.width * p.height) {
                other.insert("pixmap_width".into(), pixmap.width.into());
                other.insert("pixmap_height".into(), pixmap.height.into());
                let pixel_values: Vec<serde_json::Value> =
                    pixmap.data.iter().map(|b| (*b).into()).collect();
                other.insert("pixmap".into(), pixel_values.into());
            }
        }

        if let Some(ref tooltip) = self.tool_tip {
            if let Some(text) = tooltip.display_text() {
                other.insert("tooltip".into(), text.into());
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

async fn print_all_items(items: &ItemMap) {
    let items = items.lock().await;
    let blocks: Vec<i3bar::Block> = items.values().map(|props| props.to_block()).collect();
    if let Ok(output) = serde_json::to_string(&blocks) {
        println!("{},", output);
    }
}

const SNI_HOST_NAME: &str = "org.kde.StatusNotifierHost";
const SNI_ITEM_NAME: &str = "org.kde.StatusNotifierItem";
const SNI_WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

struct StatusNotifierHost {}

#[interface(interface = "org.kde.StatusNotifierHost")]
impl StatusNotifierHost {}

struct StatusNotifierWatcher {
    host_name: Option<String>,
    items: ItemMap,
    session: zbus::Connection,
    dbus_proxy: zbus::fdo::DBusProxy<'static>,
}

impl StatusNotifierWatcher {
    async fn fetch_properties(
        props_proxy: &zbus::fdo::PropertiesProxy<'_>,
    ) -> anyhow::Result<StatusNotifierItemProperties> {
        let raw_props = props_proxy
            .get_all(InterfaceName::try_from(SNI_ITEM_NAME).unwrap())
            .await
            .context("Failed to get properties")?;
        let props: HashMap<String, zbus::zvariant::OwnedValue> = raw_props
            .into_iter()
            .filter_map(|(k, v)| v.try_to_owned().ok().map(|v| (k, v)))
            .collect();
        Ok(StatusNotifierItemProperties::from_map(
            props_proxy.inner().destination().to_string(),
            props,
        ))
    }

    async fn stream_updates(
        session: zbus::Connection,
        dbus_proxy: zbus::fdo::DBusProxy<'static>,
        owned_emitter: SignalEmitter<'_>,
        items: ItemMap,
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

        let bus_name = zbus::names::BusName::try_from(dest.as_str())
            .context(format!("Invalid bus name: {}", dest))?
            .to_owned();

        {
            let mut items = items.lock().await;
            debug!("Inserting item: {:?}", bus_name);
            items.insert(bus_name.clone(), StatusNotifierItemProperties::default());
        }

        owned_emitter
            .status_notifier_item_registered(&service)
            .await
            .context(format!("Error emitting item registered for {}", service))?;

        let props_proxy = zbus::fdo::PropertiesProxy::new(&session, dest.clone(), path.clone())
            .await
            .context("Failed to create properties proxy")?;

        let item_props = Self::fetch_properties(&props_proxy).await?;
        debug!("Initial properties {}: {:?}", dest, item_props);

        {
            let mut items = items.lock().await;
            if let Some(item) = items.get_mut(&bus_name) {
                *item = item_props;
            }
        }
        print_all_items(&items).await;

        let inner_proxy = proxy.inner();
        let mut all_signals = inner_proxy.receive_all_signals().await?;
        let mut owner_changes = dbus_proxy.receive_name_owner_changed().await?;

        loop {
            tokio::select! {
                Some(_) = all_signals.next() => {
                    let item_props = Self::fetch_properties(&props_proxy).await?;
                    debug!("Properties updated {}: {:?}", dest, item_props);
                    {
                        let mut items = items.lock().await;
                        if let Some(item) = items.get_mut(&bus_name) {
                            *item = item_props;
                        }
                    }
                    print_all_items(&items).await;
                }
                Some(owner_change) = owner_changes.next() => {
                    let args = owner_change.args()?;
                    if args.name().as_str() == dest {
                        {
                            let mut items = items.lock().await;
                            if let Some(item) = items.get_mut(&bus_name) {
                                item.visible = false;
                                // Clearing the pixmap so it does not clutter the output.
                                item.icon_pixmap = None;
                                if let Some(ref mut tooltip) = item.tool_tip {
                                    tooltip.icon_pixmaps.clear();
                                }
                            }
                            owned_emitter
                                .status_notifier_item_unregistered(&bus_name)
                                .await?;
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

#[derive(Parser)]
#[command(
    author, version,
    about = "Status Notifier Item util for oatbar",
    long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(ValueEnum, Clone, Copy)]
enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Args)]
struct ActivateArgs {
    dbus: String,
    button: MouseButton,
    abs_x: i32,
    abs_y: i32,
}

#[derive(Subcommand)]
enum Commands {
    Activate(ActivateArgs),
}

async fn process_activate(args: ActivateArgs) -> anyhow::Result<()> {
    let session = zbus::Connection::session().await?;
    let proxy = sni_item::StatusNotifierItemProxy::builder(&session)
        .destination(args.dbus.clone())
        .context(format!("Error setting destination for {}", args.dbus))?
        .path("/StatusNotifierItem")
        .context(format!("Error setting path for {}", args.dbus))?
        .cache_properties(proxy::CacheProperties::No)
        .build()
        .await
        .context(format!("Error building proxy for {}", args.dbus))?;
    match args.button {
        MouseButton::Left => proxy.activate(args.abs_x, args.abs_y).await?,
        MouseButton::Middle => proxy.secondary_activate(args.abs_x, args.abs_y).await?,
        MouseButton::Right => proxy.context_menu(args.abs_x, args.abs_y).await?,
    }
    Ok(())
}

async fn process_streaming() -> anyhow::Result<()> {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Activate(args)) => process_activate(args).await?,
        None => process_streaming().await?,
    }
    Ok(())
}
