use anyhow::Context;
use event_listener::{Event, Listener};
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;
use zbus::names::InterfaceName;
use zbus::object_server::SignalEmitter;
use zbus::{interface, proxy};

use tracing::*;

mod sni_item;

const SNI_HOST_NAME: &str = "org.kde.StatusNotifierHost";
const SNI_ITEM_NAME: &str = "org.kde.StatusNotifierItem";
const SNI_WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

struct StatusNotifierHost {}

#[interface(interface = "org.kde.StatusNotifierHost")]
impl StatusNotifierHost {}

struct StatusNotifierWatcher {
    host_name: Option<String>,
    items: std::sync::Arc<
        tokio::sync::Mutex<
            HashMap<zbus::names::BusName<'static>, sni_item::StatusNotifierItemProxy<'static>>,
        >,
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
            tokio::sync::Mutex<
                HashMap<zbus::names::BusName<'static>, sni_item::StatusNotifierItemProxy<'static>>,
            >,
        >,
        bus_name: String,
        path: String,
        service: String,
    ) -> anyhow::Result<()> {
        let proxy = sni_item::StatusNotifierItemProxy::builder(&session)
            .destination(bus_name.clone())
            .context(format!("Error setting destination for {}", service))?
            .path(path.clone())
            .context(format!("Error setting path for {}", service))?
            .cache_properties(proxy::CacheProperties::No)
            .build()
            .await
            .context(format!("Error building proxy for {}", service))?;

        {
            let mut items = items.lock().await;

            let bus_name = zbus::names::BusName::try_from(bus_name.as_str())
                .context(format!("Error converting service {} to BusName", bus_name))?;

            debug!("Successfully registered: {}", bus_name);
            debug!("Inserting item key: {:?}", bus_name);
            items.insert(bus_name.to_owned(), proxy.clone());
        }

        owned_emitter
            .status_notifier_item_registered(&service)
            .await
            .context(format!("Error emitting item registered for {}", service))?;

        let props = zbus::fdo::PropertiesProxy::new(&session, bus_name.to_owned(), path.clone())
            .await
            .unwrap();
        let properties = props
            .get_all(InterfaceName::try_from(SNI_ITEM_NAME).unwrap())
            .await
            .unwrap();
        debug!("Properties: {:?}", properties);

        let inner_proxy = proxy.inner();

        let mut all_signals = inner_proxy.receive_all_signals().await.unwrap();
        let mut owner_changes = dbus_proxy.receive_name_owner_changed().await.unwrap();

        loop {
            tokio::select! {
                Some(_) = all_signals.next() => {
                    let properties = props
                        .get_all(InterfaceName::try_from(SNI_ITEM_NAME).unwrap())
                        .await
                        .unwrap();
                    debug!("Properties: {:?}", properties.keys().len());
                    debug!("IconName: {:?}", properties.get("IconName"));
                }
                Some(owner_change) = owner_changes.next() => {
                    let args = owner_change.args()?;
                    if args.name().as_str() == bus_name {
                        {
                            let mut items = items.lock().await;
                            let name = args.name().to_owned();
                            items.remove(&name);
                            owned_emitter.status_notifier_item_unregistered(&name).await.unwrap();
                        }
                        break;
                    }
                }
            }
        }
        debug!("Service disconnected");
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

        let (bus_name, path) = if service.starts_with("/") {
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
        debug!("Bus name: {}, Path: {}", bus_name, path);

        tokio::spawn(async move {
            if let Err(e) = Self::stream_updates(
                session,
                dbus_proxy,
                owned_emitter,
                items,
                bus_name,
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
                items: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
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
