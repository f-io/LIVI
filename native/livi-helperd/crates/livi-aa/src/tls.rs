// TLS 1.2 in frames: handshake bytes travel in SSL_HANDSHAKE control frames,
// afterwards every encrypted frame payload is one TLS record. The head unit is
// the client, the phone the server, and all channels share the one session.

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::net::IpAddr;
use std::sync::Arc;

use rustls::client::ResolvesClientCert;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, ring};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme};

use crate::cert::{HU_CERT_PEM, HU_KEY_PEM};
use crate::consts::{FLAG_FIRST, FLAG_LAST};
use crate::frame;

/// One decrypted AA message.
#[derive(Debug, PartialEq, Eq)]
pub struct Message {
    pub ch: u8,
    pub flags: u8,
    pub msg_id: u16,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum Error {
    Tls(rustls::Error),
    Io(std::io::Error),
    Cert(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Tls(e) => write!(f, "tls: {e}"),
            Error::Io(e) => write!(f, "tls io: {e}"),
            Error::Cert(e) => write!(f, "tls cert: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Takes whatever certificate the phone presents, like the main process did.
/// With the chain unverified a signature check would add nothing and would
/// refuse the v1 certificates webpki cannot parse.
#[derive(Debug)]
struct AcceptAny(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

pub fn hu_cert() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), Error> {
    let certs = rustls_pemfile::certs(&mut HU_CERT_PEM.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(Error::Io)?;
    let key = rustls_pemfile::rsa_private_keys(&mut HU_KEY_PEM.as_bytes())
        .next()
        .ok_or(Error::Cert("no key"))?
        .map_err(Error::Io)?;
    Ok((certs, PrivateKeyDer::Pkcs1(key)))
}

/// The certificate as a signing identity, built directly: the loaders would run
/// it through webpki, which refuses its v1 format.
pub fn hu_identity(provider: &CryptoProvider) -> Result<Arc<CertifiedKey>, Error> {
    let (certs, key) = hu_cert()?;
    let signing = provider.key_provider.load_private_key(key).map_err(Error::Tls)?;
    Ok(Arc::new(CertifiedKey::new(certs, signing)))
}

/// Presents the head-unit certificate whenever the phone asks for one.
#[derive(Debug)]
struct HuCert(Arc<CertifiedKey>);

impl ResolvesClientCert for HuCert {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        Some(self.0.clone())
    }

    fn has_certs(&self) -> bool {
        true
    }
}

fn client_config() -> Result<Arc<ClientConfig>, Error> {
    let provider = Arc::new(ring::default_provider());
    let algorithms = provider.signature_verification_algorithms;
    let identity = hu_identity(&provider)?;
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12])
        .map_err(Error::Tls)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAny(algorithms)))
        .with_client_cert_resolver(Arc::new(HuCert(identity)));
    Ok(Arc::new(config))
}

pub struct TlsEngine {
    conn: ClientConnection,
    /// Cleartext fragments per channel, with the flags of the first one.
    fragments: HashMap<u8, (u8, Vec<u8>)>,
}

impl TlsEngine {
    pub fn new(peer: IpAddr) -> Result<Self, Error> {
        let conn = ClientConnection::new(client_config()?, ServerName::IpAddress(peer.into()))
            .map_err(Error::Tls)?;
        Ok(Self { conn, fragments: HashMap::new() })
    }

    pub fn is_handshaking(&self) -> bool {
        self.conn.is_handshaking()
    }

    /// The TLS bytes waiting to go to the phone.
    pub fn take_output(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while self.conn.wants_write() {
            if self.conn.write_tls(&mut out).is_err() {
                break;
            }
        }
        out
    }

    /// Handshake bytes from an SSL_HANDSHAKE frame.
    pub fn inject_handshake(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.feed(bytes).map(|_| ())
    }

    /// One encrypted frame payload with the channel and flags of its frame,
    /// and the messages that completed with it.
    pub fn inject_record(&mut self, ch: u8, flags: u8, record: &[u8]) -> Result<Vec<Message>, Error> {
        let plain = self.feed(record)?;
        let mut out = Vec::new();
        if plain.is_empty() {
            return Ok(out);
        }
        let first = flags & FLAG_FIRST != 0;
        let last = flags & FLAG_LAST != 0;
        if first && last {
            out.extend(message(ch, flags, plain));
            return Ok(out);
        }
        if first {
            self.fragments.insert(ch, (flags, plain));
            return Ok(out);
        }
        let Some((first_flags, mut buf)) = self.fragments.remove(&ch) else {
            return Ok(out);
        };
        buf.extend_from_slice(&plain);
        if !last {
            self.fragments.insert(ch, (first_flags, buf));
            return Ok(out);
        }
        out.extend(message(ch, first_flags, buf));
        Ok(out)
    }

    /// Encrypts one message into wire frames.
    pub fn encrypt(&mut self, ch: u8, flags: u8, msg_id: u16, data: &[u8]) -> Result<Vec<u8>, Error> {
        let mut clear = Vec::with_capacity(2 + data.len());
        clear.extend_from_slice(&msg_id.to_be_bytes());
        clear.extend_from_slice(data);
        self.conn.writer().write_all(&clear).map_err(Error::Io)?;
        let tls = self.take_output();
        Ok(frame::encode_records(ch, flags, &tls))
    }

    /// Runs bytes through the engine and returns the cleartext they produced.
    fn feed(&mut self, mut bytes: &[u8]) -> Result<Vec<u8>, Error> {
        while !bytes.is_empty() {
            let n = self.conn.read_tls(&mut bytes).map_err(Error::Io)?;
            self.conn.process_new_packets().map_err(Error::Tls)?;
            if n == 0 {
                return Err(Error::Io(std::io::Error::other("tls input stalled")));
            }
        }
        let mut plain = Vec::new();
        match self.conn.reader().read_to_end(&mut plain) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(Error::Io(e)),
        }
        Ok(plain)
    }
}

fn message(ch: u8, flags: u8, buf: Vec<u8>) -> Option<Message> {
    if buf.len() < 2 {
        return None;
    }
    Some(Message {
        ch,
        flags,
        msg_id: u16::from_be_bytes([buf[0], buf[1]]),
        payload: buf[2..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::FLAGS_ENC_SIGNAL;
    use crate::frame::FrameSplitter;
    use rustls::server::{ClientHello, ResolvesServerCert};
    use rustls::{ServerConfig, ServerConnection};

    #[derive(Debug)]
    struct PhoneCert(Arc<CertifiedKey>);

    impl ResolvesServerCert for PhoneCert {
        fn resolve(&self, _client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
            Some(self.0.clone())
        }
    }

    /// A phone stand-in: a TLS 1.2 server with the head-unit certificate.
    fn phone() -> ServerConnection {
        let provider = Arc::new(ring::default_provider());
        let identity = hu_identity(&provider).unwrap();
        let config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS12])
            .unwrap()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(PhoneCert(identity)));
        ServerConnection::new(Arc::new(config)).unwrap()
    }

    fn pump(phone: &mut ServerConnection, bytes: &[u8]) -> Vec<u8> {
        let mut input = bytes;
        while !input.is_empty() {
            phone.read_tls(&mut input).unwrap();
            phone.process_new_packets().unwrap();
        }
        let mut out = Vec::new();
        while phone.wants_write() {
            phone.write_tls(&mut out).unwrap();
        }
        out
    }

    fn handshake() -> (TlsEngine, ServerConnection) {
        let mut hu = TlsEngine::new("10.10.0.14".parse().unwrap()).unwrap();
        let mut phone = phone();
        for _ in 0..8 {
            let to_phone = hu.take_output();
            let to_hu = pump(&mut phone, &to_phone);
            hu.inject_handshake(&to_hu).unwrap();
            if !hu.is_handshaking() && !phone.is_handshaking() {
                break;
            }
        }
        let _ = pump(&mut phone, &hu.take_output());
        assert!(!hu.is_handshaking());
        assert!(!phone.is_handshaking());
        (hu, phone)
    }

    #[test]
    fn the_handshake_completes_over_frames() {
        handshake();
    }

    #[test]
    fn a_message_from_the_phone_is_decrypted_with_its_channel() {
        let (mut hu, mut phone) = handshake();
        phone.writer().write_all(&[0x80, 0x01, 0x08, 0x07]).unwrap();
        let records = pump(&mut phone, &[]);
        let wire = frame::encode_records(3, FLAGS_ENC_SIGNAL, &records);

        let mut s = FrameSplitter::default();
        s.push(&wire);
        let f = s.next_frame().unwrap();
        let msgs = hu.inject_record(f.ch, f.flags, &f.payload).unwrap();
        assert_eq!(
            msgs,
            vec![Message { ch: 3, flags: FLAGS_ENC_SIGNAL, msg_id: 0x8001, payload: vec![8, 7] }]
        );
    }

    #[test]
    fn a_message_to_the_phone_arrives_in_clear() {
        let (mut hu, mut phone) = handshake();
        let wire = hu.encrypt(4, FLAGS_ENC_SIGNAL, 0x8004, &[8, 1, 16, 1]).unwrap();
        let mut s = FrameSplitter::default();
        s.push(&wire);
        let f = s.next_frame().unwrap();
        assert_eq!((f.ch, f.flags), (4, FLAGS_ENC_SIGNAL));
        pump(&mut phone, &f.payload);
        let mut clear = Vec::new();
        phone.reader().read_to_end(&mut clear).ok();
        assert_eq!(clear, vec![0x80, 0x04, 8, 1, 16, 1]);
    }

    #[test]
    fn fragmented_cleartext_is_reassembled() {
        let (mut hu, mut phone) = handshake();
        phone.writer().write_all(&[0x00, 0x05, 1, 2]).unwrap();
        let first = pump(&mut phone, &[]);
        phone.writer().write_all(&[3, 4]).unwrap();
        let last = pump(&mut phone, &[]);
        assert!(hu.inject_record(1, 0x09, &first).unwrap().is_empty());
        let msgs = hu.inject_record(1, 0x0a, &last).unwrap();
        assert_eq!(msgs, vec![Message { ch: 1, flags: 0x09, msg_id: 5, payload: vec![1, 2, 3, 4] }]);
    }
}
