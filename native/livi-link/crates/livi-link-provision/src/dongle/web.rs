use std::time::Duration;

use serde::Deserialize;

use super::DONGLE_HOST;

#[derive(Debug, Deserialize)]
pub struct HostInfo {
    pub update: i32,
    pub sn: String,
    pub wifi: String,
    pub otp: String,
    pub name: String,
    pub sys: HostSys,
}

#[derive(Debug, Deserialize)]
pub struct HostSys {
    pub appver: String,
    pub led: String,
}

fn cgi_url() -> String {
    format!("http://{DONGLE_HOST}/cgi-bin/index.cgi")
}

pub fn host() -> Result<HostInfo, String> {
    let url = format!("{}?id=host", cgi_url());
    let mut resp = ureq::get(&url)
        .config()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .call()
        .map_err(|e| format!("id=host: {e}"))?;
    resp.body_mut().read_json::<HostInfo>().map_err(|e| format!("id=host JSON: {e}"))
}

pub fn upload(image: &[u8]) -> Result<(), String> {
    let url = format!("{}?id=upload", cgi_url());
    let boundary = format!("----livi{}", std::process::id());
    let mut body = Vec::with_capacity(image.len() + 512);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"update.img\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(image);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let resp = ureq::post(&url)
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .config()
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .send(body.as_slice())
        .map_err(|e| format!("id=upload: {e}"))?;
    if resp.status() != 200 {
        return Err(format!("id=upload HTTP {}", resp.status()));
    }
    Ok(())
}

pub fn wait_for_update_complete(deadline_secs: u64) -> Result<(), String> {
    let start = std::time::Instant::now();
    let deadline = Duration::from_secs(deadline_secs);
    loop {
        std::thread::sleep(Duration::from_secs(2));
        // An Err means the dongle is rebooting: keep polling.
        if let Ok(info) = host() {
            if info.update == 3 {
                return Ok(());
            }
            if info.update > 3 {
                return Err(format!("update failed (code {})", info.update));
            }
        }
        if start.elapsed() > deadline {
            return Err(format!("update did not reach state 3 within {deadline_secs}s"));
        }
    }
}
