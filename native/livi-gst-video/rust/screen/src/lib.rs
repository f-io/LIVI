//! Framing and decryption of the CarPlay screen stream.
//!
//! Bytes arrive from a socket in arbitrary chunks. Each message is a 128-byte
//! header followed by its body. The header's first four bytes carry the body
//! size, little endian, the fifth carries the opcode. Frame bodies are
//! ChaCha20-Poly1305 sealed with the header as associated data and a nonce
//! counting messages from zero.

use livi_crypto_node::open_impl;
use livi_video_nal::{detect_codec, CpCodec};

pub const HEADER_LEN: usize = 128;
const OP_VIDEO_FRAME: u8 = 0;
const OP_VIDEO_CONFIG: u8 = 1;
const MAX_BODY: usize = 8 * 1024 * 1024;
const TAG_LEN: usize = 16;

/// A body size beyond `MAX_BODY`. The caller drops the connection.
#[derive(Debug, PartialEq, Eq)]
pub struct Implausible(pub usize);

/// What a stream reports as it reads: the codec and its configuration record,
/// every decrypted frame, and the first frame of a connection.
pub trait ScreenSink {
    fn on_config(&mut self, codec: CpCodec, atom: &[u8]);
    fn on_frame(&mut self, nal: &[u8]);
    fn on_started(&mut self);
}

pub struct ScreenStream {
    key: [u8; 32],
    counter: u64,
    acc: Vec<u8>,
    started: bool,
    sink: Box<dyn ScreenSink>,
}

impl ScreenStream {
    pub fn new(key: [u8; 32], sink: Box<dyn ScreenSink>) -> Self {
        Self { key, counter: 0, acc: Vec::new(), started: false, sink }
    }

    /// Forgets a half-received message and starts the nonce over.
    pub fn reset(&mut self) {
        self.acc.clear();
        self.counter = 0;
        self.started = false;
    }

    /// Takes the next chunk and reports every message it completes.
    pub fn push(&mut self, chunk: &[u8]) -> Result<(), Implausible> {
        self.acc.extend_from_slice(chunk);
        loop {
            if self.acc.len() < HEADER_LEN {
                return Ok(());
            }
            let body_size = u32::from_le_bytes([
                self.acc[0], self.acc[1], self.acc[2], self.acc[3],
            ]) as usize;
            if body_size > MAX_BODY {
                return Err(Implausible(body_size));
            }
            if self.acc.len() < HEADER_LEN + body_size {
                return Ok(());
            }
            self.handle(body_size);
            self.acc.drain(..HEADER_LEN + body_size);
        }
    }

    fn handle(&mut self, body_size: usize) {
        let opcode = self.acc[4];
        let (header, rest) = self.acc.split_at(HEADER_LEN);
        let body = &rest[..body_size];

        if opcode == OP_VIDEO_CONFIG {
            let (codec, offset) = detect_codec(body);
            self.sink.on_config(codec, &body[offset..]);
            return;
        }
        if opcode != OP_VIDEO_FRAME {
            return;
        }

        // Short bodies carry no tag and travel in the clear.
        let opened;
        let frame: &[u8] = if body.len() >= TAG_LEN {
            let Some(plain) = Self::open(&self.key, self.counter, header, body) else {
                eprintln!("[cp_screen] frame auth failed at counter {}", self.counter);
                return;
            };
            self.counter += 1;
            opened = plain;
            &opened
        } else {
            body
        };

        if !self.started {
            self.started = true;
            self.sink.on_started();
        }
        self.sink.on_frame(frame);
    }

    /// Decrypts the body of message `counter`, with the header as associated
    /// data.
    fn open(key: &[u8; 32], counter: u64, header: &[u8], body: &[u8]) -> Option<Vec<u8>> {
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&counter.to_le_bytes());
        open_impl(key, &nonce, body, header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use livi_crypto_node::seal_impl;

    const KEY: [u8; 32] = [7u8; 32];

    #[derive(Default)]
    struct Reports {
        configs: Vec<(CpCodec, Vec<u8>)>,
        frames: Vec<Vec<u8>>,
        started: usize,
    }

    /// A sink and the reader of what it collected: the stream takes one handle,
    /// the test keeps another.
    #[derive(Default, Clone)]
    struct Seen(std::rc::Rc<std::cell::RefCell<Reports>>);

    impl Seen {
        fn configs(&self) -> Vec<(CpCodec, Vec<u8>)> {
            self.0.borrow().configs.clone()
        }

        fn frames(&self) -> Vec<Vec<u8>> {
            self.0.borrow().frames.clone()
        }

        fn started(&self) -> usize {
            self.0.borrow().started
        }
    }

    impl ScreenSink for Seen {
        fn on_config(&mut self, codec: CpCodec, atom: &[u8]) {
            self.0.borrow_mut().configs.push((codec, atom.to_vec()));
        }

        fn on_frame(&mut self, nal: &[u8]) {
            self.0.borrow_mut().frames.push(nal.to_vec());
        }

        fn on_started(&mut self) {
            self.0.borrow_mut().started += 1;
        }
    }

    fn stream(seen: &Seen) -> ScreenStream {
        ScreenStream::new(KEY, Box::new(seen.clone()))
    }

    fn message(opcode: u8, body: &[u8]) -> Vec<u8> {
        let mut m = vec![0u8; HEADER_LEN];
        m[..4].copy_from_slice(&(body.len() as u32).to_le_bytes());
        m[4] = opcode;
        m.extend_from_slice(body);
        m
    }

    fn sealed_frame(counter: u64, plain: &[u8]) -> Vec<u8> {
        // the header is the associated data, so it carries its final body size
        let mut header = vec![0u8; HEADER_LEN];
        header[..4].copy_from_slice(&((plain.len() + TAG_LEN) as u32).to_le_bytes());
        header[4] = OP_VIDEO_FRAME;
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&counter.to_le_bytes());
        let body = seal_impl(&KEY, &nonce, plain, &header).unwrap();

        let mut msg = header;
        msg.extend_from_slice(&body);
        msg
    }

    #[test]
    fn a_config_message_reports_codec_and_atom() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        let mut body = vec![0u8; 4];
        body.extend_from_slice(b"hvcC");
        body.extend_from_slice(&[1, 2, 3]);

        s.push(&message(OP_VIDEO_CONFIG, &body)).unwrap();

        assert_eq!(seen.configs(), vec![(CpCodec::H265, vec![1, 2, 3])]);
    }

    #[test]
    fn a_sealed_frame_arrives_decrypted_and_announces_the_start_once() {
        let seen = Seen::default();
        let mut s = stream(&seen);

        s.push(&sealed_frame(0, b"first")).unwrap();
        s.push(&sealed_frame(1, b"second")).unwrap();

        assert_eq!(seen.started(), 1);
        assert_eq!(seen.frames(), vec![b"first".to_vec(), b"second".to_vec()]);
    }

    #[test]
    fn a_frame_that_fails_authentication_is_dropped_and_the_counter_stays() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        let mut bad = sealed_frame(0, b"first");
        *bad.last_mut().unwrap() ^= 0xff;

        s.push(&bad).unwrap();
        assert!(seen.frames().is_empty());
        assert_eq!(seen.started(), 0);

        // the next frame still counts as the first
        s.push(&sealed_frame(0, b"again")).unwrap();
        assert_eq!(seen.frames(), vec![b"again".to_vec()]);
    }

    #[test]
    fn a_body_too_short_for_a_tag_travels_in_the_clear() {
        let seen = Seen::default();
        let mut s = stream(&seen);

        s.push(&message(OP_VIDEO_FRAME, &[1, 2, 3])).unwrap();

        assert_eq!(seen.frames(), vec![vec![1, 2, 3]]);
        assert_eq!(seen.started(), 1);
    }

    #[test]
    fn a_message_split_across_chunks_is_reassembled() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        let msg = sealed_frame(0, b"split me");

        for piece in msg.chunks(7) {
            s.push(piece).unwrap();
        }

        assert_eq!(seen.frames(), vec![b"split me".to_vec()]);
    }

    #[test]
    fn several_messages_in_one_chunk_are_all_reported() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        let mut buf = sealed_frame(0, b"one");
        buf.extend(sealed_frame(1, b"two"));

        s.push(&buf).unwrap();

        assert_eq!(seen.frames(), vec![b"one".to_vec(), b"two".to_vec()]);
    }

    #[test]
    fn an_implausible_body_size_is_refused() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        let mut m = vec![0u8; HEADER_LEN];
        m[..4].copy_from_slice(&(MAX_BODY as u32 + 1).to_le_bytes());

        assert_eq!(s.push(&m), Err(Implausible(MAX_BODY + 1)));
    }

    #[test]
    fn an_unknown_opcode_is_ignored() {
        let seen = Seen::default();
        let mut s = stream(&seen);

        s.push(&message(9, &[1, 2, 3])).unwrap();

        assert!(seen.frames().is_empty());
        assert!(seen.configs().is_empty());
    }

    #[test]
    fn reset_forgets_the_half_message_and_starts_the_counter_over() {
        let seen = Seen::default();
        let mut s = stream(&seen);
        let msg = sealed_frame(0, b"dropped");
        s.push(&msg[..40]).unwrap();

        s.reset();
        s.push(&sealed_frame(0, b"fresh")).unwrap();

        assert_eq!(seen.frames(), vec![b"fresh".to_vec()]);
        assert_eq!(seen.started(), 1);
    }
}

#[cfg(target_os = "linux")]
pub mod receiver {
    use super::{ScreenSink, ScreenStream};
    use glib::IOCondition;
    use socket2::{Domain, Protocol, Socket, Type};
    use std::cell::RefCell;
    use std::io::Read;
    use std::net::TcpListener;
    use std::os::fd::{AsRawFd, RawFd};
    use std::rc::Rc;

    /// One listening port for a phone's screen stream. A second connection is
    /// refused while one is open, and every chunk goes to the stream, which
    /// answers whether the connection may stay.
    pub struct ScreenReceiver {
        listener: TcpListener,
        sources: Vec<glib::SourceId>,
        client: Rc<RefCell<Option<std::net::TcpStream>>>,
        stream: Rc<RefCell<ScreenStream>>,
    }

    fn listen_any_port() -> std::io::Result<TcpListener> {
        let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
        // dual stack: the phone may reach us over either family
        socket.set_only_v6(false)?;
        socket.set_reuse_address(true)?;
        socket.bind(&"[::]:0".parse::<std::net::SocketAddr>().unwrap().into())?;
        socket.listen(1)?;
        Ok(socket.into())
    }

    impl ScreenReceiver {
        pub fn new(key: [u8; 32], sink: Box<dyn ScreenSink>) -> std::io::Result<(Self, u16)> {
            let listener = listen_any_port()?;
            let port = listener.local_addr()?.port();
            listener.set_nonblocking(true)?;

            let mut r = Self {
                listener,
                sources: Vec::new(),
                client: Rc::new(RefCell::new(None)),
                stream: Rc::new(RefCell::new(ScreenStream::new(key, sink))),
            };
            r.watch_listener();
            Ok((r, port))
        }

        fn watch_listener(&mut self) {
            let fd = self.listener.as_raw_fd();
            let listener = self.listener.try_clone().expect("dup listener");
            let client = self.client.clone();
            let stream = self.stream.clone();

            let id = glib::unix_fd_add_local(fd, IOCondition::IN, move |_, _| {
                let Ok((sock, _)) = listener.accept() else {
                    return glib::ControlFlow::Continue;
                };
                if client.borrow().is_some() {
                    return glib::ControlFlow::Continue;
                }
                let _ = sock.set_nodelay(true);
                let _ = sock.set_nonblocking(true);
                stream.borrow_mut().reset();

                let cfd = sock.as_raw_fd();
                *client.borrow_mut() = Some(sock);
                watch_client(cfd, client.clone(), stream.clone());
                eprintln!("[cp_screen] video data connection accepted");
                glib::ControlFlow::Continue
            });
            self.sources.push(id);
        }
    }

    fn watch_client(
        fd: RawFd,
        client: Rc<RefCell<Option<std::net::TcpStream>>>,
        stream: Rc<RefCell<ScreenStream>>,
    ) {
        let cond = IOCondition::IN | IOCondition::HUP | IOCondition::ERR;
        glib::unix_fd_add_local(fd, cond, move |_, cond| {
            let drop_client = |stream: &Rc<RefCell<ScreenStream>>| {
                *client.borrow_mut() = None;
                stream.borrow_mut().reset();
                glib::ControlFlow::Break
            };
            if cond.contains(IOCondition::HUP) || cond.contains(IOCondition::ERR) {
                return drop_client(&stream);
            }

            let mut chunk = [0u8; 65536];
            let read = {
                let mut guard = client.borrow_mut();
                let Some(sock) = guard.as_mut() else { return glib::ControlFlow::Break };
                sock.read(&mut chunk)
            };
            match read {
                Ok(0) | Err(_) => drop_client(&stream),
                Ok(n) => match stream.borrow_mut().push(&chunk[..n]) {
                    Ok(()) => glib::ControlFlow::Continue,
                    Err(super::Implausible(size)) => {
                        eprintln!("[cp_screen] implausible bodySize {size}, dropping connection");
                        drop_client(&stream)
                    }
                },
            }
        });
    }

    impl Drop for ScreenReceiver {
        fn drop(&mut self) {
            for id in self.sources.drain(..) {
                id.remove();
            }
        }
    }
}
