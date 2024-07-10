mod protocols;

use merge_hashmap::{hashmap::recurse, Merge};
use protocols::DRoute;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::{task::JoinSet, time::Instant};
use zbus::{
    fdo::{DBusProxy, IntrospectableProxy},
    names::{BusName, OwnedBusName, OwnedInterfaceName},
    zvariant::OwnedObjectPath,
    Connection,
};
use zbus_xml::Node;

async fn get_unique_names(connection: &Connection) -> Vec<OwnedBusName> {
    let dbus_proxy = DBusProxy::new(connection).await.unwrap();
    dbus_proxy
        .list_names()
        .await
        .unwrap()
        .into_iter()
        .filter(|n| matches!(n.inner(), BusName::Unique(_)))
        .collect()
}
// async fn get_names(connection: &Connection) -> Vec<OwnedBusName> {
//     let dbus_proxy = DBusProxy::new(connection).await.unwrap();
//     dbus_proxy.list_names().await.unwrap()
// }

async fn introspect(
    connection: &Connection,
    bus_name: OwnedBusName,
    object_path: OwnedObjectPath,
) -> color_eyre::eyre::Result<Node<'static>> {
    let proxy = IntrospectableProxy::builder(connection)
        .destination(bus_name)?
        .path(object_path)?
        .build()
        .await?;
    let xml = proxy.introspect().await?;
    zbus_xml::Node::from_reader(xml.as_bytes()).map_err(Into::into)
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct InterfaceUse {
    bus: OwnedBusName,
    object: OwnedObjectPath,
}
#[derive(Debug, Default)]
pub struct InterfaceUses(HashSet<InterfaceUse>);
impl Merge for InterfaceUses {
    fn merge(&mut self, other: Self) {
        self.0.extend(other.0)
    }
}

fn get_interface_uses(
    connection: Arc<Connection>,
    bus_name: OwnedBusName,
    object_path: OwnedObjectPath,
) -> Pin<
    Box<
        dyn Future<Output = color_eyre::eyre::Result<HashMap<OwnedInterfaceName, InterfaceUses>>>
            + Send,
    >,
> {
    Box::pin(async move {
        let introspection = tokio::time::timeout(
            Duration::from_millis(10),
            introspect(&connection, bus_name.clone(), object_path.clone()),
        )
        .await??;

        let mut interface_uses: HashMap<OwnedInterfaceName, InterfaceUses> = HashMap::new();
        for interface in introspection.interfaces() {
            interface_uses
                .entry(interface.name().into())
                .or_default()
                .0
                .insert(InterfaceUse {
                    bus: bus_name.clone(),
                    object: object_path.clone(),
                });
        }

        let mut join_set = JoinSet::new();
        for object in introspection.nodes() {
            let Some(name) = object.name().map(ToString::to_string) else {
                continue;
            };
            let name = object_path.to_string() + "/" + &name;
            let Ok(object_path) = OwnedObjectPath::try_from(name.replace("//", "/")) else {
                continue;
            };
            // dbg!(&object_path);
            join_set.spawn(get_interface_uses(
                connection.clone(),
                bus_name.clone(),
                object_path,
            ));
        }
        while let Some(uses) = join_set.join_next().await {
            let Ok(Ok(uses)) = uses else { continue };
            // dbg!(&uses);
            recurse(&mut interface_uses, uses);
        }
        Ok(interface_uses)
    })
}

#[tokio::main(flavor = "current_thread")]
// #[tokio::main]
async fn main() {
    color_eyre::install().unwrap();

    let connection = Arc::new(Connection::session().await.unwrap());
    let root_object = OwnedObjectPath::try_from("/").unwrap();

    let start = Instant::now();

    // dbg!(get_unique_names(&connection).await);
    let unique_names = get_unique_names(&connection);
    let mut join_set = JoinSet::new();
    for name in unique_names.await {
        // let root_object = match name.inner() {
        //     BusName::WellKnown(n) => {
        //         let Ok(path) = OwnedObjectPath::try_from("/".to_string() + &n.replace('.', "/"))
        //         else {
        //             continue;
        //         };
        //         dbg!(path)
        //     }
        //     BusName::Unique(_) => root_object.clone(),
        // };
        let connection = connection.clone();
        join_set.spawn(get_interface_uses(connection, name, root_object.clone()));
    }

    let mut interface_uses = HashMap::new();
    while let Some(uses) = join_set.join_next().await {
        let Ok(Ok(uses)) = uses else { continue };
        recurse(&mut interface_uses, uses);
    }
    dbg!(start.elapsed());
    dbg!(interface_uses.len());

    connection
        .object_server()
        .at("/com/fyralabs/DRoute", DRoute)
        .await
        .unwrap();
    connection
        .request_name("com.fyralabs.DRoute")
        .await
        .unwrap();
}
