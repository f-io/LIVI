// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};

pub struct Arrivals {
    current: Arc<Mutex<Option<TcpStream>>>,
    tx: Sender<TcpStream>,
}

pub struct Hosts {
    rx: Receiver<TcpStream>,
    pending: Option<TcpStream>,
}

pub fn queue() -> (Arrivals, Hosts) {
    let (tx, rx) = channel();
    (
        Arrivals {
            current: Arc::default(),
            tx,
        },
        Hosts { rx, pending: None },
    )
}

impl Arrivals {
    pub fn arrive(&self, stream: TcpStream) {
        let held = stream.try_clone().ok();
        let before = std::mem::replace(&mut *self.current.lock().unwrap(), held);
        if let Some(old) = before {
            let _ = old.shutdown(Shutdown::Both);
        }
        let _ = self.tx.send(stream);
    }
}

impl Hosts {
    pub fn next(&mut self) -> Option<TcpStream> {
        let mut newest = match self.pending.take() {
            Some(s) => s,
            None => self.rx.recv().ok()?,
        };
        while let Ok(newer) = self.rx.try_recv() {
            newest = newer;
        }
        Some(newest)
    }

    pub fn waiting(&mut self) -> bool {
        if self.pending.is_some() {
            return true;
        }
        match self.rx.try_recv() {
            Ok(s) => {
                self.pending = Some(s);
                true
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    fn connection(listener: &TcpListener) -> (TcpStream, TcpStream) {
        let host = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (dongle, _) = listener.accept().unwrap();
        host.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        (host, dongle)
    }

    fn hung_up(mut host: &TcpStream) -> bool {
        let mut b = [0u8; 1];
        matches!(host.read(&mut b), Ok(0) | Err(_))
    }

    fn same(a: &TcpStream, b: &TcpStream) -> bool {
        a.peer_addr().unwrap() == b.local_addr().unwrap()
    }

    #[test]
    fn a_new_host_ends_the_session_of_the_one_before() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (arrivals, mut hosts) = queue();
        let (old_host, old) = connection(&listener);
        arrivals.arrive(old);
        let mut session = hosts.next().unwrap();
        assert!(!hosts.waiting());

        let (new_host, new) = connection(&listener);
        arrivals.arrive(new);
        let mut b = [0u8; 1];
        assert_eq!(
            session.read(&mut b).unwrap(),
            0,
            "the old session sees its end"
        );
        assert!(hung_up(&old_host));
        assert!(hosts.waiting());
        assert!(same(&new_host, &hosts.next().unwrap()));
        assert!(!hosts.waiting());
    }

    #[test]
    fn of_several_queued_hosts_only_the_newest_gets_in() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (arrivals, mut hosts) = queue();
        let (first_host, first) = connection(&listener);
        let (second_host, second) = connection(&listener);
        let (third_host, third) = connection(&listener);
        for s in [first, second, third] {
            arrivals.arrive(s);
        }
        assert!(same(&third_host, &hosts.next().unwrap()));
        assert!(hung_up(&first_host));
        assert!(hung_up(&second_host));
    }

    #[test]
    fn a_host_that_left_on_its_own_does_not_hang_up_the_next() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (arrivals, mut hosts) = queue();
        let (old_host, old) = connection(&listener);
        arrivals.arrive(old);
        drop(hosts.next().unwrap());
        drop(old_host);

        let (new_host, new) = connection(&listener);
        arrivals.arrive(new);
        let mut session = hosts.next().unwrap();
        session.write_all(b"ok").unwrap();
        let mut b = [0u8; 2];
        (&new_host).read_exact(&mut b).unwrap();
        assert_eq!(&b, b"ok");
    }

    #[test]
    fn no_hosts_left_ends_the_queue() {
        let (arrivals, mut hosts) = queue();
        drop(arrivals);
        assert!(!hosts.waiting());
        assert!(hosts.next().is_none());
    }
}
