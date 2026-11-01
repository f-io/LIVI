// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use super::*;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;

fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    b.extend_from_slice(kind);
    b.extend_from_slice(body);
    b
}

fn full(version_flags: u32, rest: &[u8]) -> Vec<u8> {
    let mut b = version_flags.to_be_bytes().to_vec();
    b.extend_from_slice(rest);
    b
}

fn words(ws: &[u32]) -> Vec<u8> {
    ws.iter().flat_map(|w| w.to_be_bytes()).collect()
}

const AVCC: [u8; 11] = [1, 0x64, 0, 0x1f, 0xFF, 0xE1, 0, 2, 0x67, 0x64, 0];

fn trak(id: u32, handler: &[u8; 4], entry: &[u8; 4], avcc: &[u8], v1: bool) -> Vec<u8> {
    let tkhd = if v1 {
        full(1 << 24, &[vec![0; 16], words(&[id])].concat())
    } else {
        full(0, &[vec![0; 8], words(&[id])].concat())
    };
    let mdhd = if v1 {
        full(1 << 24, &[vec![0; 16], words(&[90_000])].concat())
    } else {
        full(0, &[vec![0; 8], words(&[90_000])].concat())
    };
    let hdlr = full(0, &[vec![0; 4], handler.to_vec()].concat());
    let mut sample_entry = vec![0u8; 78];
    sample_entry[24..26].copy_from_slice(&720u16.to_be_bytes());
    sample_entry[26..28].copy_from_slice(&576u16.to_be_bytes());
    sample_entry.extend(mp4_box(b"avcC", avcc));
    let stsd = full(0, &[words(&[1]), mp4_box(entry, &sample_entry)].concat());
    let stbl = mp4_box(b"stbl", &mp4_box(b"stsd", &stsd));
    let minf = mp4_box(b"minf", &stbl);
    let mdia = mp4_box(
        b"mdia",
        &[mp4_box(b"mdhd", &mdhd), mp4_box(b"hdlr", &hdlr), minf].concat(),
    );
    mp4_box(b"trak", &[mp4_box(b"tkhd", &tkhd), mdia].concat())
}

fn init() -> Vec<u8> {
    let trex = full(0, &words(&[2, 1, 3600, 5, 0]));
    let other_trex = full(0, &words(&[9, 1, 1, 1, 0]));
    let moov = [
        trak(1, b"soun", b"mp4a", &AVCC, false),
        trak(2, b"vide", b"avc1", &AVCC, true),
        mp4_box(
            b"mvex",
            &[mp4_box(b"trex", &other_trex), mp4_box(b"trex", &trex)].concat(),
        ),
    ]
    .concat();
    [mp4_box(b"ftyp", b"iso6"), mp4_box(b"moov", &moov)].concat()
}

fn track() -> Track {
    read_init(&init()).unwrap()
}

#[test]
fn the_init_segment_names_the_video_track() {
    let t = track();
    assert_eq!(
        (t.id, t.timescale, t.width, t.height),
        (2, 90_000, 720, 576)
    );
    assert_eq!(t.avcc, AVCC);
    assert_eq!((t.default_duration, t.default_size), (3600, 5));

    let short_lengths = [1, 0x64, 0, 0x1f, 0xFD, 0xE1, 0, 2, 0x67, 0x64, 0];
    let moov = mp4_box(b"moov", &trak(1, b"vide", b"avc1", &short_lengths, false));
    assert_eq!(read_init(&moov), None);
    let hevc = mp4_box(b"moov", &trak(1, b"vide", b"hvc1", &AVCC, false));
    assert_eq!(read_init(&hevc), None);
    let only = mp4_box(b"moov", &trak(4, b"vide", b"avc3", &AVCC, false));
    let t = read_init(&only).unwrap();
    assert_eq!((t.id, t.default_duration), (4, 0));
    assert_eq!(read_init(b"\0\0\0\x10moov"), None);
}

fn moof(track: u32, tfhd_flags: u32, tfhd_rest: &[u32], trun_flags: u32, trun: &[u32]) -> Vec<u8> {
    let tfhd = full(tfhd_flags, &words(&[&[track], tfhd_rest].concat()));
    let trun = full(trun_flags, &words(trun));
    let traf = mp4_box(
        b"traf",
        &[mp4_box(b"tfhd", &tfhd), mp4_box(b"trun", &trun)].concat(),
    );
    mp4_box(b"moof", &traf)
}

#[test]
fn a_segment_yields_its_samples_from_mdat() {
    let t = track();
    let payload = b"AAAABBBCC".to_vec();
    let probe = moof(
        2,
        0x020000,
        &[],
        0x301 | 0x400 | 0x800,
        &[3, 0, 1, 4, 0, 0, 2, 3, 0, 0, 3, 2, 0, 0],
    );
    let offset = probe.len() as u32 + 8;
    let first = moof(
        2,
        0x020000,
        &[],
        0x301 | 0x400 | 0x800,
        &[3, offset, 1, 4, 0, 0, 2, 3, 0, 0, 3, 2, 0, 0],
    );
    let segment = [first, mp4_box(b"mdat", &payload)].concat();
    let samples = read_segment(&segment, &t);
    let got: Vec<(&[u8], u32)> = samples
        .iter()
        .map(|s| (s.data.as_slice(), s.duration))
        .collect();
    assert_eq!(got, [(&b"AAAA"[..], 1), (b"BBB", 2), (b"CC", 3)]);

    let probe = moof(2, 0x18, &[10, 2], 0x4, &[2, 0]);
    let segment = [probe, b"XXYY".to_vec()].concat();
    let samples = read_segment(&segment, &t);
    assert_eq!(samples.len(), 2);
    assert_eq!((samples[0].data.len(), samples[1].duration), (2, 10));

    let base = moof(2, 0x1 | 0x2, &[0, 0, 1], 0x201, &[1, 0, 4]);
    let at = base.len();
    let base = moof(2, 0x1 | 0x2, &[0, at as u32, 1], 0x201, &[1, 0, 4]);
    let segment = [base, b"WXYZ".to_vec()].concat();
    let samples = read_segment(&segment, &t);
    assert_eq!(
        (samples[0].data.as_slice(), samples[0].duration),
        (&b"WXYZ"[..], 3600)
    );

    let other = moof(7, 0, &[], 0x200, &[1, 4]);
    assert!(read_segment(&[other, b"WXYZ".to_vec()].concat(), &t).is_empty());
    let past_end = moof(2, 0, &[], 0x200, &[1, 400]);
    assert!(read_segment(&past_end, &t).is_empty());
    let cut = moof(2, 0, &[], 0x300, &[2, 1]);
    assert!(read_segment(&cut, &t).is_empty());
    let no_offset = moof(2, 0, &[], 0x1, &[1]);
    assert!(read_segment(&no_offset, &t).is_empty());
    let no_tfhd = mp4_box(b"moof", &mp4_box(b"traf", &[]));
    assert!(read_segment(&no_tfhd, &t).is_empty());
    let wide = [
        1u32.to_be_bytes().to_vec(),
        b"mdat".to_vec(),
        16u64.to_be_bytes().to_vec(),
    ]
    .concat();
    let rest = [
        wide,
        mp4_box(b"free", b""),
        0u32.to_be_bytes().to_vec(),
        b"skip".to_vec(),
    ]
    .concat();
    assert!(read_segment(&rest, &t).is_empty());
}

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
                let answer = match files.iter().find(|(p, _, _)| *p == path) {
                    Some((_, body, true)) => {
                        let mut a = format!(
                            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
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
    assert_eq!(get(&format!("{base}/a")).await.unwrap(), b"plain");
    assert_eq!(get(&format!("{base}/b")).await.unwrap(), b"chunks");
    assert!(
        get(&format!("{base}/c"))
            .await
            .unwrap_err()
            .to_string()
            .contains("404")
    );
    assert!(get("ftp://x/a").await.is_err());
    assert!(get("https:///a").await.is_err());
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = silent.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = silent.accept().await.unwrap();
        let _ = sock.read(&mut [0u8; 1024]).await;
        sock.write_all(b"garbage").await.unwrap();
    });
    assert!(
        get(&format!("http://{at}"))
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
    assert_eq!(get(&format!("http://{at}/x")).await.unwrap(), b"body");
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
        get(&format!("http://{at}/x"))
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

#[tokio::test]
async fn a_server_that_can_hold_the_playlist_is_asked_for_the_next_segment() {
    let (base, seen) = keep_alive_server().await;
    let mut stream = Stream {
        url: format!("{base}/index.m3u8"),
        track: track(),
        next: 7,
        give_up: GIVE_UP,
        client: Client::default(),
        init: "init.mp4".into(),
        discontinuity: 0,
        block_reload: true,
    };
    stream.playlist().await.unwrap();
    assert_eq!(seen.lock().unwrap().1, ["/index.m3u8?_HLS_msn=7"]);
    assert!(
        !stream.block_reload,
        "an answer without the flag turns holding off"
    );
    stream.playlist().await.unwrap();
    assert_eq!(seen.lock().unwrap().1.last().unwrap(), "/index.m3u8");
}

#[tokio::test]
async fn a_stream_that_starts_over_is_reported() {
    let playlist = |map: &str, seq: u64, disc: u64| {
        format!(
            "#EXT-X-MEDIA-SEQUENCE:{seq}\n#EXT-X-DISCONTINUITY-SEQUENCE:{disc}\n\
             #EXT-X-MAP:URI=\"{map}\"\n#EXTINF:1,\nseg.m4s\n"
        )
        .into_bytes()
    };
    let base = server(vec![
        ("/same.m3u8", playlist("init.mp4", 5, 0), false),
        ("/new_init.m3u8", playlist("init_1.mp4", 5, 0), false),
        ("/new_disc.m3u8", playlist("init.mp4", 5, 1), false),
        ("/init.mp4", init(), false),
        ("/seg.m4s", media_segment(&[b"x"]), false),
    ])
    .await;
    let mut stream = Stream::open(&format!("{base}/same.m3u8")).await.unwrap();
    assert!(stream.fetch_next().await.unwrap().is_some());
    for gone in ["new_init", "new_disc"] {
        stream.url = format!("{base}/{gone}.m3u8");
        assert!(
            started_over(&stream.fetch_next().await.unwrap_err()),
            "{gone}"
        );
    }
    stream.url = format!("{base}/same.m3u8");
    stream.next = 50;
    let e = stream.fetch_next().await.unwrap_err();
    assert!(started_over(&e) && e.to_string().contains("started over"));
    assert!(!started_over(&io::Error::other("other")));

    stream.next = 5;
    let (tx, _rx) = mpsc::channel(1);
    stream.url = format!("{base}/new_init.m3u8");
    assert!(started_over(&stream.run(tx).await.unwrap_err()));
}

fn media_segment(samples: &[&[u8]]) -> Vec<u8> {
    let mut entries = Vec::new();
    for s in samples {
        entries.extend_from_slice(&[s.len() as u32]);
    }
    let probe = moof(
        2,
        0x020008,
        &[1800],
        0x201,
        &[&[samples.len() as u32, 0][..], &entries].concat(),
    );
    let offset = probe.len() as u32 + 8;
    let moof = moof(
        2,
        0x020008,
        &[1800],
        0x201,
        &[&[samples.len() as u32, offset][..], &entries].concat(),
    );
    [moof, mp4_box(b"mdat", &samples.concat())].concat()
}

#[tokio::test]
async fn a_live_stream_plays_from_behind_the_newest_segment() {
    let playlist = b"#EXT-X-MEDIA-SEQUENCE:5\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:1,\nseg_5.m4s\n#EXTINF:1,\nseg_6.m4s\n".to_vec();
    let base = server(vec![
        ("/live/index.m3u8", playlist, false),
        ("/live/init.mp4", init(), false),
        ("/live/seg_5.m4s", media_segment(&[b"old"]), false),
        ("/live/seg_6.m4s", media_segment(&[b"one", b"two"]), true),
    ])
    .await;
    let stream = Stream::open(&format!("{base}/live/index.m3u8"))
        .await
        .unwrap();
    assert_eq!(stream.track.width, 720);
    let (tx, mut rx) = mpsc::channel(1);
    let run = tokio::spawn(stream.run(tx));
    let started = Instant::now();
    assert_eq!(rx.recv().await.unwrap(), b"old");
    assert_eq!(rx.recv().await.unwrap(), b"one");
    assert_eq!(rx.recv().await.unwrap(), b"two");
    assert!(
        started.elapsed() >= Duration::from_millis(40),
        "paced by the sample duration"
    );
    tokio::time::sleep(POLL * 3).await;
    drop(rx);
    run.abort();

    let no_map = server(vec![("/p.m3u8", b"#EXTM3U\nseg.m4s\n".to_vec(), false)]).await;
    assert!(Stream::open(&format!("{no_map}/p.m3u8")).await.is_err());
    let bad_init = server(vec![
        ("/p.m3u8", b"#EXT-X-MAP:URI=\"i.mp4\"\n".to_vec(), false),
        ("/i.mp4", b"nothing".to_vec(), false),
    ])
    .await;
    assert!(Stream::open(&format!("{bad_init}/p.m3u8")).await.is_err());
}

#[tokio::test]
async fn the_run_ends_when_nobody_takes_frames_or_the_server_fails() {
    let playlist = b"#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-MAP:URI=\"init.mp4\"\nseg.m4s\n".to_vec();
    let base = server(vec![
        ("/index.m3u8", playlist.clone(), false),
        ("/init.mp4", init(), false),
        ("/seg.m4s", media_segment(&[b"a"]), false),
    ])
    .await;
    let stream = Stream::open(&format!("{base}/index.m3u8")).await.unwrap();
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    stream.run(tx).await.unwrap();

    let gone = server(vec![
        ("/index.m3u8", playlist, false),
        ("/init.mp4", init(), false),
    ])
    .await;
    let mut stream = Stream::open(&format!("{gone}/index.m3u8")).await.unwrap();
    stream.next = 0;
    stream.give_up = POLL * 2;
    let (tx, _rx) = mpsc::channel(1);
    assert!(
        stream
            .run(tx)
            .await
            .unwrap_err()
            .to_string()
            .contains("404")
    );
}
