// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use zbus::Connection;

use crate::state::HelperState;

pub const PING_TIMEOUT: Duration = Duration::from_secs(1);
const FAST_INTERVAL: Duration = Duration::from_secs(1);
const FAST_ATTEMPTS: u32 = 15;
const SLOW_INTERVAL: Duration = Duration::from_secs(30);
const STALE: Duration = Duration::from_secs(10);

pub enum Link {
    Up,
    Down,
    Unknown,
}

pub trait Paging {
    fn ready(&self) -> bool;

    fn page(&self, mac: &str, wake: Option<&str>) -> impl Future<Output = Option<bool>> + Send;

    fn alive(&self) -> bool {
        true
    }

    fn wanted(&self, _wake: Option<&str>) -> bool {
        true
    }

    fn served(&self, _mac: &str, _wake: Option<&str>) -> bool {
        false
    }

    fn link(&self, _mac: &str) -> impl Future<Output = Link> + Send {
        async { Link::Down }
    }

    fn poke(&self, _mac: &str, _wake: Option<&str>) -> impl Future<Output = ()> + Send {
        async {}
    }

    fn disconnect(&self, _mac: &str) -> impl Future<Output = ()> + Send {
        async {}
    }
}

pub async fn run(radio: impl Paging, state: Arc<HelperState>) {
    let mut turn: usize = 0;
    let mut attempts: HashMap<String, u32> = HashMap::new();
    let mut next_try: HashMap<String, Instant> = HashMap::new();
    let mut stale_since: HashMap<String, Instant> = HashMap::new();

    while radio.alive() {
        tokio::time::sleep(FAST_INTERVAL).await;
        let targets: Vec<_> = state
            .reconnect_targets()
            .into_iter()
            .filter(|(_, wake)| radio.wanted(wake.as_deref()))
            .collect();
        let listed = |mac: &String| targets.iter().any(|(m, _)| m == mac);
        attempts.retain(|mac, _| listed(mac));
        next_try.retain(|mac, _| listed(mac));
        stale_since.retain(|mac, _| listed(mac));
        if targets.is_empty() {
            continue;
        }
        let (mac, wake) = &targets[turn % targets.len()];
        turn = turn.wrapping_add(1);
        let wake = wake.as_deref();

        if state.link_active(mac) || radio.served(mac, wake) {
            attempts.remove(mac);
            next_try.remove(mac);
            stale_since.remove(mac);
            continue;
        }
        match radio.link(mac).await {
            Link::Unknown => continue,
            Link::Up => {
                attempts.remove(mac);
                next_try.remove(mac);
                let since = *stale_since.entry(mac.clone()).or_insert_with(Instant::now);
                if since.elapsed() >= STALE {
                    stale_since.remove(mac);
                    println!("[bt] {mac} connected but no session, disconnecting");
                    radio.disconnect(mac).await;
                } else {
                    radio.poke(mac, wake).await;
                }
                continue;
            }
            Link::Down => {
                stale_since.remove(mac);
            }
        }

        if next_try.get(mac).is_some_and(|t| Instant::now() < *t) || !radio.ready() {
            continue;
        }
        match radio.page(mac, wake).await {
            None => continue,
            Some(true) => {
                attempts.remove(mac);
                next_try.remove(mac);
                continue;
            }
            Some(false) => {}
        }
        let n = attempts.entry(mac.clone()).or_insert(0);
        *n += 1;
        if *n >= FAST_ATTEMPTS {
            next_try.insert(mac.clone(), Instant::now() + SLOW_INTERVAL);
        }
    }
}

pub struct Bluez {
    pub conn: Connection,
    pub adapter: String,
    pub ap_iface: String,
}

impl Paging for Bluez {
    fn ready(&self) -> bool {
        crate::net::wlan_link_local(&self.ap_iface).is_some()
    }

    async fn link(&self, mac: &str) -> Link {
        match device_connected(&self.conn, &device_path(&self.adapter, mac)).await {
            Some(true) => Link::Up,
            Some(false) => Link::Down,
            None => Link::Unknown,
        }
    }

    async fn poke(&self, mac: &str, wake: Option<&str>) {
        if wake.is_some() {
            let path = device_path(&self.adapter, mac);
            let _ = tokio::time::timeout(PING_TIMEOUT, page(&self.conn, &path, wake)).await;
        }
    }

    async fn page(&self, mac: &str, wake: Option<&str>) -> Option<bool> {
        println!("[bt] paging {mac}");
        let path = device_path(&self.adapter, mac);
        match tokio::time::timeout(PING_TIMEOUT, page(&self.conn, &path, wake)).await {
            Ok(Ok(())) => {
                println!("[bt] {mac} connected");
                Some(true)
            }
            Ok(Err(e)) => {
                println!("[bt] {mac} page failed: {e}");
                Some(false)
            }
            Err(_) => Some(false),
        }
    }

    async fn disconnect(&self, mac: &str) {
        let _ = disconnect(&self.conn, &device_path(&self.adapter, mac)).await;
    }
}

fn device_path(adapter: &str, mac: &str) -> String {
    format!(
        "/org/bluez/{}/dev_{}",
        adapter,
        mac.replace(':', "_").to_uppercase()
    )
}

async fn device_connected(conn: &Connection, path: &str) -> Option<bool> {
    let reply = conn
        .call_method(
            Some("org.bluez"),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.bluez.Device1", "Connected"),
        )
        .await
        .ok()?;
    let value: zbus::OwnedVariant = reply.body().deserialize().ok()?;
    bool::try_from(value).ok()
}

async fn page(conn: &Connection, path: &str, uuid: Option<&str>) -> Result<(), zbus::Error> {
    match uuid {
        Some(uuid) => {
            conn.call_method(
                Some("org.bluez"),
                path,
                Some("org.bluez.Device1"),
                "ConnectProfile",
                &(uuid,),
            )
            .await?;
        }
        None => {
            conn.call_method(
                Some("org.bluez"),
                path,
                Some("org.bluez.Device1"),
                "Connect",
                &(),
            )
            .await?;
        }
    }
    Ok(())
}

async fn disconnect(conn: &Connection, path: &str) -> Result<(), zbus::Error> {
    conn.call_method(
        Some("org.bluez"),
        path,
        Some("org.bluez.Device1"),
        "Disconnect",
        &(),
    )
    .await?;
    Ok(())
}
