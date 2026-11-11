// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::{caps, route};

pub(crate) const INDEX_HTML: &str = include_str!("../web/index.html");

pub(crate) fn serve_forever() -> std::io::Result<()> {
    let port = caps().port;
    let addr = SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0);
    let listener = TcpListener::bind(SocketAddr::V6(addr))?;
    eprintln!("[livi-httpd] listening on [::]:{port} (dual-stack)");

    // SAFETY: SIG_IGN installs no handler code.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    for conn in listener.incoming() {
        match conn {
            Ok(s) => {
                thread::spawn(|| handle(s));
            }
            Err(e) => eprintln!("[livi-httpd] accept: {e}"),
        }
    }
    Ok(())
}

pub(crate) fn handle(mut sock: TcpStream) {
    let _ = sock.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(30)));

    let Some((method, path, content_length, body)) = read_request(&sock) else {
        return;
    };

    let (status, ctype, resp) = match body {
        Ok(body) => route(&method, &path, body.as_deref(), content_length),
        Err(TooLarge) => (S_413, T_JSON, err_json("body too large")),
    };
    let _ = write_response(&mut sock, status, ctype, &resp);
}

pub(crate) struct TooLarge;

pub(crate) const SMALL_BODY: u64 = 64 << 10;

pub(crate) fn max_body() -> u64 {
    caps().flash.mtd.iter().map(|slot| slot.size).sum::<u64>() + SMALL_BODY
}

pub(crate) type Body = Result<Option<Vec<u8>>, TooLarge>;

pub(crate) fn read_request(sock: &TcpStream) -> Option<(String, String, u64, Body)> {
    let mut reader = BufReader::new(sock);

    let mut first = String::new();
    reader.read_line(&mut first).ok()?;
    if first.is_empty() {
        return None;
    }
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut content_length: u64 = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).ok()?;
        if n <= 2 {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(u64::MAX);
        }
    }

    let body = if content_length > max_body() {
        Err(TooLarge)
    } else if content_length > 0 {
        let mut buf = vec![0u8; content_length as usize];
        reader.read_exact(&mut buf).ok()?;
        Ok(Some(buf))
    } else {
        Ok(None)
    };

    Some((method, path, content_length, body))
}

pub(crate) fn ok_json(msg: &str) -> Vec<u8> {
    format!(r#"{{"ok":true,"message":"{}"}}"#, js(msg)).into_bytes()
}

pub(crate) fn err_json(msg: &str) -> Vec<u8> {
    format!(r#"{{"ok":false,"error":"{}"}}"#, js(msg)).into_bytes()
}

pub(crate) fn json_str(hay: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let i = hay.find(&needle)?;
    let after = &hay[i + needle.len()..];
    let colon = after.find(':')?;
    let rest = &after[colon + 1..];
    let start = rest.find('"')?;
    let rest = &rest[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub(crate) fn json_num(hay: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let i = hay.find(&needle)?;
    let after = &hay[i + needle.len()..];
    let colon = after.find(':')?;
    let rest = after[colon + 1..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some(rest[..end].to_string())
}

pub(crate) fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        match b {
            b'+' => out.push(' '),
            b'%' => {
                let h = it.next().unwrap_or(b'0') as char;
                let l = it.next().unwrap_or(b'0') as char;
                if let Ok(v) = u8::from_str_radix(&format!("{h}{l}"), 16) {
                    out.push(v as char);
                }
            }
            _ => out.push(b as char),
        }
    }
    out
}

pub(crate) fn js(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn write_response(
    w: &mut TcpStream,
    status: &str,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    );
    w.write_all(header.as_bytes())?;
    w.write_all(body)?;
    Ok(())
}

pub(crate) const S_200: &str = "200 OK";

pub(crate) const S_400: &str = "400 Bad Request";

pub(crate) const S_404: &str = "404 Not Found";

pub(crate) const S_413: &str = "413 Content Too Large";

pub(crate) const S_500: &str = "500 Internal Server Error";

pub(crate) const T_HTML: &str = "text/html; charset=utf-8";

pub(crate) const T_JSON: &str = "application/json";

pub(crate) const T_TEXT: &str = "text/plain; charset=utf-8";
