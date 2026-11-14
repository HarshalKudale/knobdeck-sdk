//! D-Bus connections (session and system, opened once) and small call helpers.

use crate::{Error, Result};
use std::sync::{Mutex, OnceLock};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{DynamicType, OwnedValue};

fn cached(cell: &'static OnceLock<Mutex<Option<Connection>>>, open: fn() -> zbus::Result<Connection>) -> Result<Connection> {
    let mut slot = cell.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if let Some(c) = slot.as_ref() {
        return Ok(c.clone());
    }
    let c = open().map_err(|e| Error::msg(format!("D-Bus: {e}")))?;
    *slot = Some(c.clone());
    Ok(c)
}

/// The session bus (one connection for the plugin's lifetime).
pub fn session() -> Result<Connection> {
    static S: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();
    cached(&S, Connection::session)
}

/// The system bus (logind, NetworkManager, BlueZ, power profiles).
pub fn system() -> Result<Connection> {
    static S: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();
    cached(&S, Connection::system)
}

pub fn err(e: zbus::Error) -> Error {
    // A missing service reads better than zbus' full error chain.
    match &e {
        zbus::Error::MethodError(name, msg, _) => Error::msg(msg.clone().unwrap_or_else(|| name.to_string())),
        zbus::Error::FDO(f) => Error::msg(f.to_string()),
        _ => Error::msg(format!("D-Bus: {e}")),
    }
}

/// Whether `name` has an owner on the session bus.
pub fn session_has_owner(name: &str) -> bool {
    has_owner(session(), name)
}

/// Whether `name` has an owner on the system bus.
pub fn system_has_owner(name: &str) -> bool {
    has_owner(system(), name)
}

fn has_owner(conn: Result<Connection>, name: &str) -> bool {
    let Ok(conn) = conn else { return false };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return false };
    let Ok(name) = zbus::names::BusName::try_from(name) else { return false };
    dbus.name_has_owner(name).unwrap_or(false)
}

/// Every name on the session bus starting with `prefix` (e.g. MPRIS players).
pub fn session_names(prefix: &str) -> Vec<String> {
    let Ok(conn) = session() else { return vec![] };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return vec![] };
    dbus.list_names().unwrap_or_default().into_iter().map(|n| n.to_string()).filter(|n| n.starts_with(prefix)).collect()
}

/// Call a method and decode its reply.
pub fn call<A, R>(conn: &Connection, dest: &str, path: &str, iface: &str, method: &str, args: &A) -> Result<R>
where
    A: serde::Serialize + DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    let reply = conn.call_method(Some(dest), path, Some(iface), method, args).map_err(err)?;
    reply.body().deserialize::<R>().map_err(err)
}

/// Call a method, ignoring its reply.
pub fn call0<A>(conn: &Connection, dest: &str, path: &str, iface: &str, method: &str, args: &A) -> Result
where
    A: serde::Serialize + DynamicType,
{
    conn.call_method(Some(dest), path, Some(iface), method, args).map_err(err)?;
    Ok(())
}

pub fn proxy<'a>(conn: &Connection, dest: &'a str, path: &'a str, iface: &'a str) -> Result<Proxy<'a>> {
    Proxy::new(conn, dest, path, iface).map_err(err)
}

/// Read a property.
pub fn get<T>(conn: &Connection, dest: &str, path: &str, iface: &str, prop: &str) -> Result<T>
where
    T: TryFrom<OwnedValue>,
    T::Error: Into<zbus::Error>,
{
    proxy(conn, dest, path, iface)?.get_property::<T>(prop).map_err(err)
}

/// Write a property.
pub fn set<'v, T>(conn: &Connection, dest: &str, path: &str, iface: &str, prop: &str, value: T) -> Result
where
    T: 'v + serde::Serialize + DynamicType + Into<zbus::zvariant::Value<'v>>,
{
    proxy(conn, dest, path, iface)?.set_property(prop, value).map_err(|e| Error::msg(format!("{prop}: {e}")))
}
