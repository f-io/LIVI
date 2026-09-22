//! The socket helpers stream media into. Every connection carries records for
//! the sink on the main loop, like the screen receiver does it. A session and a
//! call bridge can be connected at the same time.

use std::cell::RefCell;
use std::io::Read;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;

use glib::IOCondition;
use livi_host_proto::feed::Framer;

use crate::MediaSink;

type Clients = Rc<RefCell<Vec<(u64, UnixStream)>>>;
type Sink = Rc<RefCell<Box<dyn MediaSink>>>;

pub struct FeedListener {
    path: String,
    listener: UnixListener,
    sources: Vec<glib::SourceId>,
    clients: Clients,
}

impl FeedListener {
    pub fn new(path: &str, sink: Box<dyn MediaSink>) -> std::io::Result<Self> {
        let _ = std::fs::remove_file(path);
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        let mut l = Self {
            path: path.to_owned(),
            listener,
            sources: Vec::new(),
            clients: Rc::new(RefCell::new(Vec::new())),
        };
        l.watch_listener(Rc::new(RefCell::new(sink)));
        Ok(l)
    }

    fn watch_listener(&mut self, sink: Sink) {
        let fd = self.listener.as_raw_fd();
        let listener = self.listener.try_clone().expect("dup listener");
        let clients = self.clients.clone();
        let mut next = 0u64;
        let id = glib::unix_fd_add_local(fd, IOCondition::IN, move |_, _| {
            let Ok((sock, _)) = listener.accept() else {
                return glib::ControlFlow::Continue;
            };
            let _ = sock.set_nonblocking(true);
            next += 1;
            let cfd = sock.as_raw_fd();
            clients.borrow_mut().push((next, sock));
            watch_client(cfd, next, clients.clone(), sink.clone());
            eprintln!("[feed] connection accepted");
            glib::ControlFlow::Continue
        });
        self.sources.push(id);
    }
}

fn watch_client(fd: RawFd, generation: u64, clients: Clients, sink: Sink) {
    let cond = IOCondition::IN | IOCondition::HUP | IOCondition::ERR;
    let mut framer = Framer::new();
    glib::unix_fd_add_local(fd, cond, move |_, cond| {
        let mut chunk = [0u8; 65536];
        let read = {
            let mut guard = clients.borrow_mut();
            match guard.iter_mut().find(|(g, _)| *g == generation) {
                Some((_, sock)) => {
                    if cond.contains(IOCondition::IN) {
                        sock.read(&mut chunk).ok().filter(|n| *n > 0)
                    } else {
                        None
                    }
                }
                // already gone
                None => return glib::ControlFlow::Break,
            }
        };
        match read {
            Some(n) => {
                framer.push(&chunk[..n]);
                let mut s = sink.borrow_mut();
                while let Some(record) = framer.next_record() {
                    s.on_record(record);
                }
                glib::ControlFlow::Continue
            }
            None => {
                clients.borrow_mut().retain(|(g, _)| *g != generation);
                eprintln!("[feed] connection closed");
                glib::ControlFlow::Break
            }
        }
    });
}

impl Drop for FeedListener {
    fn drop(&mut self) {
        for id in self.sources.drain(..) {
            id.remove();
        }
        self.clients.borrow_mut().clear();
        let _ = std::fs::remove_file(&self.path);
    }
}
