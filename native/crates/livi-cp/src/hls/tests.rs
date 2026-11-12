// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio::time::Instant;

#[test]
fn the_playlist_lists_whole_segments() {
    let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:108\n#EXT-X-MAP:URI=\"init_0.mp4\"\n\
                #EXTINF:1.000,\nseg_108.m4s\n#EXT-X-PART:DURATION=0.2,URI=\"part_109_0.m4s\"\n\
                #EXTINF:1.000,\nseg_109.m4s\n";
    let list = read_playlist(text);
    assert_eq!(list.init.as_deref(), Some("init_0.mp4"));
    assert_eq!(list.first, 108);
    assert_eq!(list.segments, ["seg_108.m4s", "seg_109.m4s"]);
    assert_eq!((list.discontinuity, list.block_reload), (0, false));
    assert_eq!(
        read_playlist("#EXT-X-MEDIA-SEQUENCE:x"),
        Playlist::default()
    );

    let live = read_playlist(
        "#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=YES,PART-HOLD-BACK=0.468\n\
         #EXT-X-DISCONTINUITY-SEQUENCE:2\n",
    );
    assert_eq!((live.discontinuity, live.block_reload), (2, true));
    assert!(!read_playlist("#EXT-X-SERVER-CONTROL:CAN-BLOCK-RELOAD=NO\n").block_reload);
    assert_eq!(with_query("http://h/a.m3u8", "x=1"), "http://h/a.m3u8?x=1");
    assert_eq!(
        with_query("http://h/a.m3u8?y=2", "x=1"),
        "http://h/a.m3u8?y=2&x=1"
    );
}

#[test]
fn uris_resolve_against_the_playlist() {
    let base = "http://127.0.0.1:9/mirror/index.m3u8?x=1";
    assert_eq!(
        resolve(base, "seg_1.m4s"),
        "http://127.0.0.1:9/mirror/seg_1.m4s"
    );
    assert_eq!(
        resolve(base, "/other/a.mp4"),
        "http://127.0.0.1:9/other/a.mp4"
    );
    assert_eq!(resolve(base, "http://h/a"), "http://h/a");
    assert_eq!(resolve("http://127.0.0.1:9", "a"), "http://127.0.0.1:9/a");
    assert_eq!(resolve("http://127.0.0.1:9/", "a"), "http://127.0.0.1:9/a");
}

#[test]
fn chunked_bodies_are_joined() {
    assert_eq!(
        dechunk(b"3\r\nabc\r\n2;x\r\nde\r\n0\r\n\r\n"),
        Some(b"abcde".to_vec())
    );
    assert_eq!(dechunk(b"zz\r\n"), None);
    assert_eq!(dechunk(b"5\r\nab"), None);
}

async fn server(files: Vec<(&'static str, Vec<u8>, bool)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let files = files.clone();
            tokio::spawn(async move {
                let mut ask = vec![0u8; 1024];
                let n = sock.read(&mut ask).await.unwrap_or(0);
                let ask = String::from_utf8_lossy(&ask[..n]).to_string();
                let path = ask.split(' ').nth(1).unwrap_or_default().to_string();
                let file = files.iter().find(|(p, _, _)| *p == path);
                let answer = match file {
                    Some((_, body, true)) => {
                        let mut a = format!(
                            "HTTP/1.1 200 OK\r\nTransfer-Encoding:  chunked\r\nConnection: keep-alive\r\n\r\n{:x}\r\n",
                            body.len()
                        )
                        .into_bytes();
                        a.extend_from_slice(body);
                        a.extend_from_slice(b"\r\n0\r\n\r\n");
                        a
                    }
                    Some((_, body, false)) => {
                        let mut a =
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
                                .into_bytes();
                        a.extend_from_slice(body);
                        a
                    }
                    None => b"HTTP/1.1 404 Not Found\r\n\r\n".to_vec(),
                };
                let _ = sock.write_all(&answer).await;
                if matches!(file, Some((_, _, true))) {
                    let _ = sock.read(&mut [0u8; 1]).await;
                }
            });
        }
    });
    base
}

#[tokio::test]
async fn get_reads_plain_and_chunked_answers() {
    let base = server(vec![
        ("/a", b"plain".to_vec(), false),
        ("/b", b"chunks".to_vec(), true),
    ])
    .await;
    assert_eq!(
        Client::default().get(&format!("{base}/a")).await.unwrap(),
        b"plain"
    );
    assert_eq!(
        Client::default().get(&format!("{base}/b")).await.unwrap(),
        b"chunks"
    );
    assert!(
        Client::default()
            .get(&format!("{base}/c"))
            .await
            .unwrap_err()
            .to_string()
            .contains("404")
    );
    assert!(Client::default().get("ftp://x/a").await.is_err());
    assert!(Client::default().get("https:///a").await.is_err());
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = silent.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = silent.accept().await.unwrap();
        let _ = sock.read(&mut [0u8; 1024]).await;
        sock.write_all(b"garbage").await.unwrap();
    });
    assert!(
        Client::default()
            .get(&format!("http://{at}"))
            .await
            .unwrap_err()
            .to_string()
            .contains("no HTTP")
    );

    let open = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = open.local_addr().unwrap();
    let held = tokio::spawn(async move {
        let (mut sock, _) = open.accept().await.unwrap();
        let _ = sock.read(&mut [0u8; 1024]).await;
        sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody")
            .await
            .unwrap();
        tokio::time::sleep(FETCH_TIMEOUT * 2).await;
    });
    let started = Instant::now();
    assert_eq!(
        Client::default()
            .get(&format!("http://{at}/x"))
            .await
            .unwrap(),
        b"body"
    );
    assert!(started.elapsed() < FETCH_TIMEOUT);
    held.abort();

    let short = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = short.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = short.accept().await.unwrap();
        let _ = sock.read(&mut [0u8; 1024]).await;
        sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nshort")
            .await
            .unwrap();
    });
    assert!(
        Client::default()
            .get(&format!("http://{at}/x"))
            .await
            .unwrap_err()
            .to_string()
            .contains("cut short")
    );
}

async fn keep_alive_server() -> (String, Arc<Mutex<(usize, Vec<String>)>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new((0, Vec::new())));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            log.lock().unwrap().0 += 1;
            tokio::spawn(echo(sock, log.clone()));
        }
    });
    (base, seen)
}

type Seen = Arc<Mutex<(usize, Vec<String>)>>;

async fn echo<S: AsyncRead + AsyncWrite + Unpin>(mut sock: S, log: Seen) {
    let mut buf = vec![0u8; 4096];
    while let Ok(n) = sock.read(&mut buf).await {
        if n == 0 {
            return;
        }
        let ask = String::from_utf8_lossy(&buf[..n]).to_string();
        let path = ask.split(' ').nth(1).unwrap_or_default().to_string();
        log.lock().unwrap().1.push(path.clone());
        let answer = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{path}",
            path.len()
        );
        if sock.write_all(answer.as_bytes()).await.is_err() {
            return;
        }
    }
}

#[test]
fn a_target_names_where_to_connect_and_whom_to_trust() {
    let t = Target::of("https://cdn.example/v/a.m3u8?x=1").unwrap();
    assert_eq!(
        (t.tls, t.host, t.path),
        (true, "cdn.example", "/v/a.m3u8?x=1")
    );
    assert_eq!(
        (t.origin(), t.address(), t.name()),
        (
            "https://cdn.example".into(),
            "cdn.example:443".into(),
            "cdn.example"
        )
    );
    let t = Target::of("http://[::1]:8080").unwrap();
    assert_eq!(
        (t.path, t.address(), t.name()),
        ("/", "[::1]:8080".into(), "::1")
    );
    let t = Target::of("https://[::1]/a").unwrap();
    assert_eq!((t.address(), t.name()), ("[::1]:443".into(), "::1"));
    assert_eq!(Target::of("http://h:81/").unwrap().name(), "h");
    assert_eq!(Target::of("http://h/").unwrap().address(), "h:80");
    assert_eq!(
        resolve("https://h/a/b.m3u8", "//cdn/x.ts"),
        "https://cdn/x.ts"
    );
}

#[tokio::test]
async fn https_is_checked_against_the_trusted_roots_and_kept_open() {
    let (acceptor, trust) = crate::test_tls::ca();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("https://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::new(Mutex::new((0, Vec::new())));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            log.lock().unwrap().0 += 1;
            let ssl = openssl::ssl::Ssl::new(acceptor.context()).unwrap();
            let log = log.clone();
            tokio::spawn(async move {
                let mut tls = SslStream::new(ssl, tcp).unwrap();
                if Pin::new(&mut tls).accept().await.is_ok() {
                    echo(tls, log).await;
                }
            });
        }
    });

    let mut client = Client::default().trusting(trust);
    for path in ["/a", "/b"] {
        assert_eq!(
            client.get(&format!("{base}{path}")).await.unwrap(),
            path.as_bytes()
        );
    }
    assert_eq!(seen.lock().unwrap().0, 1);
    assert!(
        Client::default().get(&format!("{base}/a")).await.is_err(),
        "unknown root"
    );
}

#[tokio::test]
async fn one_connection_carries_every_request() {
    let (base, seen) = keep_alive_server().await;
    let mut client = Client::default();
    for path in ["/a", "/b", "/c"] {
        assert_eq!(
            client.get(&format!("{base}{path}")).await.unwrap(),
            path.as_bytes()
        );
    }
    assert_eq!(seen.lock().unwrap().0, 1);

    let closing = server(vec![("/a", b"A".to_vec(), false)]).await;
    let mut client = Client::default();
    for _ in 0..3 {
        assert_eq!(client.get(&format!("{closing}/a")).await.unwrap(), b"A");
    }
    let other = server(vec![("/b", b"B".to_vec(), true)]).await;
    assert_eq!(client.get(&format!("{other}/b")).await.unwrap(), b"B");
}
