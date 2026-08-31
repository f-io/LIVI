//! Framing and decryption of the CarPlay screen stream.
//!
//! Bytes arrive from a socket in arbitrary chunks. Each message is a 128-byte
//! header — whose first four bytes carry the body size, little endian, and
//! whose fifth byte carries the opcode — followed by that body. Frame bodies
//! are ChaCha20-Poly1305 sealed with the header as associated data and a nonce
//! counting messages from zero.

use core::ffi::c_void;
use livi_crypto_node::cabi::livi_chacha20poly1305_open;
use livi_video_nal::{detect_codec, CpCodec};

pub const HEADER_LEN: usize = 128;
const OP_VIDEO_FRAME: u8 = 0;
const OP_VIDEO_CONFIG: u8 = 1;
const MAX_BODY: usize = 8 * 1024 * 1024;
const TAG_LEN: usize = 16;

/// Matches `CpScreenCallbacks` in cp_screen_receiver.h.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CpScreenCallbacks {
    pub on_config:
        Option<unsafe extern "C" fn(codec: i32, atom: *const u8, len: usize, user: *mut c_void)>,
    pub on_frame: Option<unsafe extern "C" fn(nal: *const u8, len: usize, user: *mut c_void)>,
    pub on_started: Option<unsafe extern "C" fn(user: *mut c_void)>,
    pub user: *mut c_void,
}

/// A body size beyond `MAX_BODY`; the caller drops the connection.
#[derive(Debug, PartialEq, Eq)]
pub struct Implausible(pub usize);

pub struct ScreenStream {
    key: [u8; 32],
    counter: u64,
    acc: Vec<u8>,
    plain: Vec<u8>,
    started: bool,
    cb: CpScreenCallbacks,
}

impl ScreenStream {
    pub fn new(key: [u8; 32], cb: CpScreenCallbacks) -> Self {
        Self { key, counter: 0, acc: Vec::new(), plain: Vec::new(), started: false, cb }
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
            if let Some(on_config) = self.cb.on_config {
                let atom = &body[offset..];
                unsafe {
                    on_config(codec as i32, atom.as_ptr(), atom.len(), self.cb.user);
                }
            }
            return;
        }
        if opcode != OP_VIDEO_FRAME {
            return;
        }

        // Short bodies carry no tag and travel in the clear.
        let nal_len = if body.len() >= TAG_LEN {
            match Self::open(&self.key, self.counter, header, body, &mut self.plain) {
                Some(len) => {
                    self.counter += 1;
                    len
                }
                None => {
                    eprintln!("[cp_screen] frame auth failed at counter {}", self.counter);
                    return;
                }
            }
        } else {
            self.plain.clear();
            self.plain.extend_from_slice(body);
            body.len()
        };

        if !self.started {
            self.started = true;
            if let Some(on_started) = self.cb.on_started {
                unsafe { on_started(self.cb.user) }
            }
        }
        if let Some(on_frame) = self.cb.on_frame {
            unsafe { on_frame(self.plain.as_ptr(), nal_len, self.cb.user) }
        }
    }

    /// Decrypts into `plain`, which grows to hold the plaintext and is reused
    /// across frames. Returns the plaintext length.
    fn open(
        key: &[u8; 32],
        counter: u64,
        header: &[u8],
        body: &[u8],
        plain: &mut Vec<u8>,
    ) -> Option<usize> {
        let mut nonce = [0u8; 12];
        nonce[4..].copy_from_slice(&counter.to_le_bytes());

        plain.clear();
        plain.resize(body.len() - TAG_LEN, 0);

        let mut got = 0usize;
        let ok = unsafe {
            livi_chacha20poly1305_open(
                plain.as_mut_ptr(),
                &mut got,
                key.as_ptr(),
                nonce.as_ptr(),
                header.as_ptr(),
                header.len(),
                body.as_ptr(),
                body.len(),
            )
        };
        if ok != 0 {
            return None;
        }
        Some(got)
    }
}

/// Matches `CpCodec`; kept so callers need not name the nal crate.
pub use livi_video_nal::CpCodec as Codec;
const _: () = assert!(CpCodec::H264 as i32 == 0);

/// # Safety
/// `key` points to 32 readable bytes, `cb` to a live callbacks struct. The
/// returned stream is owned by the caller and freed with
/// `cp_screen_stream_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_screen_stream_new(
    key: *const u8,
    cb: *const CpScreenCallbacks,
) -> *mut ScreenStream {
    if key.is_null() || cb.is_null() {
        return core::ptr::null_mut();
    }
    let mut k = [0u8; 32];
    unsafe { core::ptr::copy_nonoverlapping(key, k.as_mut_ptr(), 32) };
    Box::into_raw(Box::new(ScreenStream::new(k, unsafe { *cb })))
}

/// # Safety
/// `s` comes from `cp_screen_stream_new` and is not freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_screen_stream_reset(s: *mut ScreenStream) {
    if let Some(s) = unsafe { s.as_mut() } {
        s.reset();
    }
}

/// Returns 0 when the caller may read on, -1 when the message stream is broken
/// and the connection should be dropped.
///
/// # Safety
/// `s` comes from `cp_screen_stream_new`, `data` points to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_screen_stream_push(
    s: *mut ScreenStream,
    data: *const u8,
    len: usize,
) -> i32 {
    let Some(s) = (unsafe { s.as_mut() }) else {
        return -1;
    };
    let chunk = if data.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    };
    match s.push(chunk) {
        Ok(()) => 0,
        Err(Implausible(size)) => {
            eprintln!("[cp_screen] implausible bodySize {size}, dropping connection");
            -1
        }
    }
}

/// # Safety
/// `s` comes from `cp_screen_stream_new` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cp_screen_stream_free(s: *mut ScreenStream) {
    if !s.is_null() {
        drop(unsafe { Box::from_raw(s) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use livi_crypto_node::seal_impl;

    const KEY: [u8; 32] = [7u8; 32];

    #[derive(Default)]
    struct Seen {
        configs: Vec<(i32, Vec<u8>)>,
        frames: Vec<Vec<u8>>,
        started: usize,
    }

    unsafe extern "C" fn on_config(codec: i32, atom: *const u8, len: usize, user: *mut c_void) {
        let seen = unsafe { &mut *(user as *mut Seen) };
        let bytes = unsafe { core::slice::from_raw_parts(atom, len) };
        seen.configs.push((codec, bytes.to_vec()));
    }

    unsafe extern "C" fn on_frame(nal: *const u8, len: usize, user: *mut c_void) {
        let seen = unsafe { &mut *(user as *mut Seen) };
        seen.frames.push(unsafe { core::slice::from_raw_parts(nal, len) }.to_vec());
    }

    unsafe extern "C" fn on_started(user: *mut c_void) {
        unsafe { &mut *(user as *mut Seen) }.started += 1;
    }

    fn stream(seen: &mut Seen) -> ScreenStream {
        ScreenStream::new(
            KEY,
            CpScreenCallbacks {
                on_config: Some(on_config),
                on_frame: Some(on_frame),
                on_started: Some(on_started),
                user: seen as *mut Seen as *mut c_void,
            },
        )
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
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);
        let mut body = vec![0u8; 4];
        body.extend_from_slice(b"hvcC");
        body.extend_from_slice(&[1, 2, 3]);

        s.push(&message(OP_VIDEO_CONFIG, &body)).unwrap();

        assert_eq!(seen.configs, vec![(CpCodec::H265 as i32, vec![1, 2, 3])]);
    }

    #[test]
    fn a_sealed_frame_arrives_decrypted_and_announces_the_start_once() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);

        s.push(&sealed_frame(0, b"first")).unwrap();
        s.push(&sealed_frame(1, b"second")).unwrap();

        assert_eq!(seen.started, 1);
        assert_eq!(seen.frames, vec![b"first".to_vec(), b"second".to_vec()]);
    }

    #[test]
    fn a_frame_that_fails_authentication_is_dropped_and_the_counter_stays() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);
        let mut bad = sealed_frame(0, b"first");
        *bad.last_mut().unwrap() ^= 0xff;

        s.push(&bad).unwrap();
        assert!(seen.frames.is_empty());
        assert_eq!(seen.started, 0);

        // the next frame still counts as the first
        s.push(&sealed_frame(0, b"again")).unwrap();
        assert_eq!(seen.frames, vec![b"again".to_vec()]);
    }

    #[test]
    fn a_body_too_short_for_a_tag_travels_in_the_clear() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);

        s.push(&message(OP_VIDEO_FRAME, &[1, 2, 3])).unwrap();

        assert_eq!(seen.frames, vec![vec![1, 2, 3]]);
        assert_eq!(seen.started, 1);
    }

    #[test]
    fn a_message_split_across_chunks_is_reassembled() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);
        let msg = sealed_frame(0, b"split me");

        for piece in msg.chunks(7) {
            s.push(piece).unwrap();
        }

        assert_eq!(seen.frames, vec![b"split me".to_vec()]);
    }

    #[test]
    fn several_messages_in_one_chunk_are_all_reported() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);
        let mut buf = sealed_frame(0, b"one");
        buf.extend(sealed_frame(1, b"two"));

        s.push(&buf).unwrap();

        assert_eq!(seen.frames, vec![b"one".to_vec(), b"two".to_vec()]);
    }

    #[test]
    fn an_implausible_body_size_is_refused() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);
        let mut m = vec![0u8; HEADER_LEN];
        m[..4].copy_from_slice(&(MAX_BODY as u32 + 1).to_le_bytes());

        assert_eq!(s.push(&m), Err(Implausible(MAX_BODY + 1)));
    }

    #[test]
    fn an_unknown_opcode_is_ignored() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);

        s.push(&message(9, &[1, 2, 3])).unwrap();

        assert!(seen.frames.is_empty());
        assert!(seen.configs.is_empty());
    }

    #[test]
    fn reset_forgets_the_half_message_and_starts_the_counter_over() {
        let mut seen = Seen::default();
        let mut s = stream(&mut seen);
        let msg = sealed_frame(0, b"dropped");
        s.push(&msg[..40]).unwrap();

        s.reset();
        s.push(&sealed_frame(0, b"fresh")).unwrap();

        assert_eq!(seen.frames, vec![b"fresh".to_vec()]);
        assert_eq!(seen.started, 1);
    }

    #[test]
    fn a_stream_without_callbacks_stays_quiet() {
        let mut s = ScreenStream::new(
            KEY,
            CpScreenCallbacks {
                on_config: None,
                on_frame: None,
                on_started: None,
                user: core::ptr::null_mut(),
            },
        );

        s.push(&message(OP_VIDEO_CONFIG, b"xxxxhvcC1")).unwrap();
        s.push(&sealed_frame(0, b"quiet")).unwrap();
    }
}

#[cfg(target_os = "linux")]
mod receiver {
    use super::{CpScreenCallbacks, ScreenStream};
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
        pub fn new(key: [u8; 32], cb: CpScreenCallbacks) -> std::io::Result<(Self, u16)> {
            let listener = listen_any_port()?;
            let port = listener.local_addr()?.port();
            listener.set_nonblocking(true)?;

            let mut r = Self {
                listener,
                sources: Vec::new(),
                client: Rc::new(RefCell::new(None)),
                stream: Rc::new(RefCell::new(ScreenStream::new(key, cb))),
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

    /// # Safety
    /// `key` points to 32 readable bytes, `cb` to a live callbacks struct,
    /// `out_port` is writable or null. The result is freed with
    /// `cp_screen_receiver_free`.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn cp_screen_receiver_new(
        key: *const u8,
        cb: *const CpScreenCallbacks,
        out_port: *mut u16,
    ) -> *mut ScreenReceiver {
        if key.is_null() || cb.is_null() {
            return core::ptr::null_mut();
        }
        let mut k = [0u8; 32];
        unsafe { core::ptr::copy_nonoverlapping(key, k.as_mut_ptr(), 32) };
        match ScreenReceiver::new(k, unsafe { *cb }) {
            Ok((r, port)) => {
                if !out_port.is_null() {
                    unsafe { *out_port = port };
                }
                Box::into_raw(Box::new(r))
            }
            Err(e) => {
                eprintln!("[cp_screen] cannot listen: {e}");
                core::ptr::null_mut()
            }
        }
    }

    /// # Safety
    /// `r` comes from `cp_screen_receiver_new` and is not used afterwards.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn cp_screen_receiver_free(r: *mut ScreenReceiver) {
        if !r.is_null() {
            drop(unsafe { Box::from_raw(r) });
        }
    }
}
