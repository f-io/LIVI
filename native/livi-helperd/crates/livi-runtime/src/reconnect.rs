// Round-robins paired phones back over Bluetooth after a restart.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use zbus::Connection;

use crate::state::HelperState;

const PING_TIMEOUT: Duration = Duration::from_secs(1);
const FAST_INTERVAL: Duration = Duration::from_secs(1);
const FAST_ATTEMPTS: u32 = 15;
const SLOW_INTERVAL: Duration = Duration::from_secs(30);
const STALE: Duration = Duration::from_secs(10);

pub async fn run(conn: Connection, adapter: String, state: Arc<HelperState>) {
    let mut rr: usize = 0;
    let mut attempts: HashMap<String, u32> = HashMap::new();
    let mut next_try: HashMap<String, Instant> = HashMap::new();
    let mut stale_since: HashMap<String, Instant> = HashMap::new();

    loop {
        tokio::time::sleep(FAST_INTERVAL).await;
        let targets = state.reconnect_targets();
        attempts.retain(|mac, _| targets.iter().any(|(m, _)| m == mac));
        next_try.retain(|mac, _| targets.iter().any(|(m, _)| m == mac));
        stale_since.retain(|mac, _| targets.iter().any(|(m, _)| m == mac));
        if targets.is_empty() {
            continue;
        }

        let (mac, uuid) = targets[rr % targets.len()].clone();
        rr = rr.wrapping_add(1);

        let path = device_path(&adapter, &mac);
        // No BlueZ device object for this MAC
        let connected = match device_connected(&conn, &path).await {
            Some(c) => c,
            None => continue,
        };

        if connected {
            // Out of the rotation: nudge the profile, disconnect after STALE so it re-pages.
            attempts.remove(&mac);
            next_try.remove(&mac);
            let stale = match stale_since.entry(mac.clone()) {
                Entry::Vacant(e) => {
                    e.insert(Instant::now());
                    false
                }
                Entry::Occupied(e) => {
                    e.get().elapsed() >= STALE && {
                        e.remove();
                        true
                    }
                }
            };
            if stale {
                println!("[cp] reconnect: {mac} connected but no session, disconnecting");
                let _ = disconnect(&conn, &path).await;
            } else if uuid.is_some() {
                let _ = tokio::time::timeout(PING_TIMEOUT, page(&conn, &path, uuid.as_deref())).await;
            }
            continue;
        }

        stale_since.remove(&mac);

        // Absent phone: after the fast pokes, only poke every SLOW_INTERVAL.
        if next_try.get(&mac).is_some_and(|t| Instant::now() < *t) {
            continue;
        }

        println!("[cp] reconnect: paging {mac}");
        match tokio::time::timeout(PING_TIMEOUT, page(&conn, &path, uuid.as_deref())).await {
            Ok(Ok(())) => {
                println!("[cp] reconnect: {mac} connected");
                attempts.remove(&mac);
                next_try.remove(&mac);
                continue;
            }
            Ok(Err(e)) => println!("[cp] reconnect: {mac} page failed: {e}"),
            // The poke woke it; the next pass catches it.
            Err(_) => {}
        }

        // After FAST_ATTEMPTS quick pokes, drop to the slow poll.
        let n = attempts.entry(mac.clone()).or_insert(0);
        *n += 1;
        if *n >= FAST_ATTEMPTS {
            next_try.insert(mac, Instant::now() + SLOW_INTERVAL);
        }
    }
}

fn device_path(adapter: &str, mac: &str) -> String {
    format!("/org/bluez/{}/dev_{}", adapter, mac.replace(':', "_").to_uppercase())
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
    let value: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
    bool::try_from(value).ok()
}

async fn page(conn: &Connection, path: &str, uuid: Option<&str>) -> Result<(), zbus::Error> {
    match uuid {
        Some(uuid) => {
            conn.call_method(Some("org.bluez"), path, Some("org.bluez.Device1"), "ConnectProfile", &(uuid,))
                .await?;
        }
        None => {
            conn.call_method(Some("org.bluez"), path, Some("org.bluez.Device1"), "Connect", &())
                .await?;
        }
    }
    Ok(())
}

async fn disconnect(conn: &Connection, path: &str) -> Result<(), zbus::Error> {
    conn.call_method(Some("org.bluez"), path, Some("org.bluez.Device1"), "Disconnect", &()).await?;
    Ok(())
}
