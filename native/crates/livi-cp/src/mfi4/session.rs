// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use super::cert::{self, Trust, Unpacked};
use super::keys::{self, Channel, Ephemeral, Keys, POINT, TAG};
use super::wire::{self, CLEAR, HEADER, Message, TUNNELED};
use crate::auth_setup::MfiSigner;
use crate::crypto::{random_bytes, sha256};

mod id {
    pub const PHONE_SETUP: u16 = 0x5100;
    pub const PHONE_SETUP_FAILED: u16 = 0x5102;
    pub const PHONE_START: u16 = 0x5111;
    pub const PHONE_WANTS_CERT: u16 = 0x5112;
    pub const PHONE_CERT: u16 = 0x5113;
    pub const PHONE_TUNNEL_READ: u16 = 0x5163;
    pub const PHONE_TUNNEL_DATA: u16 = 0x5164;
    pub const PHONE_GAVE_UP: u16 = 0x51FA;
    pub const PHONE_FAILED: u16 = 0x51FE;
    pub const PHONE_SUCCEEDED: u16 = 0x51FF;

    pub const SETUP: u16 = 0x5201;
    pub const SETUP_FAILED: u16 = 0x5202;
    pub const WANT_CERT: u16 = 0x5212;
    pub const CERT: u16 = 0x5213;
    pub const FINISH: u16 = 0x5214;
    pub const TUNNEL_DATA: u16 = 0x5264;
    pub const TUNNEL_FAILED: u16 = 0x5265;
    pub const TUNNEL_TAKEN: u16 = 0x5266;
    pub const GAVE_UP: u16 = 0x52FA;
    pub const RESET: u16 = 0x52FD;
    pub const FAILED: u16 = 0x52FE;
    pub const SUCCEEDED: u16 = 0x52FF;

    pub const TUNNELED_FROM_PHONE: u16 = 0x5500;
    pub const TUNNELED_FROM_US: u16 = 0x5501;
}

const LOWEST_VERSION: u8 = 2;
const OUR_VERSION: u8 = 3;
const PHONE_PROVES_AS_DEVICE: u8 = 1;
const WE_RESPOND: u8 = 2;
const OUR_CHAIN_IS_ACCESSORY: u8 = 0;
const OUR_CERT_IS_3_0_LEAF: u8 = 0;
const PHONE_INTERMEDIATE: [u8; 2] = [4, 5];
const PHONE_LEAF: u8 = 6;
const MAX_AUTH_TYPES: usize = 4;
const MAX_CAPABILITIES: usize = 3;
const HASH: usize = 32;
const SIGNATURE: usize = 64;
const PREFIX: usize = 16;
const PAIRING_DATA: u8 = 1;

pub type Tunnel<'a> = &'a mut (dyn FnMut(&[u8]) -> Result<Vec<u8>, String> + Send);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Fresh,
    SetUp,
    AwaitingPhoneCert,
    Finished,
    Proven,
    Ended,
}

pub struct Session {
    step: Step,
    our_nonce: u16,
    session_id: u16,
    ephemeral: Option<Ephemeral>,
    phone_point: [u8; POINT],
    version: u8,
    phone_auth_types: Vec<u8>,
    phone_capabilities: Vec<u8>,
    keys: Option<Keys>,
    phone_hash: [u8; HASH],
    phone_signature: [u8; SIGNATURE],
    our_prefix: [u8; PREFIX],
    our_cert: Vec<u8>,
    our_hash: [u8; HASH],
    trust: Trust,
    leaf_waiting: Option<Unpacked>,
    tunnel_out: Vec<u8>,
    pub binds_pairing: bool,
}

enum Fault {
    Malformed(String),
    Operation(String),
}

impl Session {
    pub fn new(root_dir: Option<&Path>) -> Self {
        let trust = Trust::new(root_dir);
        if !trust.rooted() {
            println!(
                "[cp] mfi4: no Apple accessory root, the phone's certificate chain goes unchecked"
            );
        }
        Self {
            step: Step::Fresh,
            our_nonce: 0,
            session_id: 0,
            ephemeral: None,
            phone_point: [0; POINT],
            version: 0,
            phone_auth_types: Vec::new(),
            phone_capabilities: Vec::new(),
            keys: None,
            phone_hash: [0; HASH],
            phone_signature: [0; SIGNATURE],
            our_prefix: [0; PREFIX],
            our_cert: Vec::new(),
            our_hash: [0; HASH],
            trust,
            leaf_waiting: None,
            tunnel_out: Vec::new(),
            binds_pairing: false,
        }
    }

    pub fn proven(&self) -> bool {
        self.step == Step::Proven
    }

    pub async fn handle(
        &mut self,
        body: &[u8],
        chip: &impl MfiSigner,
        tunnel: Tunnel<'_>,
    ) -> Result<Vec<u8>, String> {
        let asked = wire::peek(body).map_or(0, |(_, id)| id);
        let answer = self.answer(body, chip, tunnel).await;
        match &answer {
            Ok(out) => {
                let to = wire::peek(out).map_or(0, |(_, id)| id);
                println!("[cp] mfi4 {asked:#06x} -> {to:#06x} ({:?})", self.step);
            }
            Err(e) => println!("[cp] mfi4 {asked:#06x}: {e}"),
        }
        answer
    }

    async fn answer(
        &mut self,
        body: &[u8],
        chip: &impl MfiSigner,
        tunnel: Tunnel<'_>,
    ) -> Result<Vec<u8>, String> {
        if self.step == Step::Ended {
            return Err("the authentication on this connection is over".into());
        }
        let (start, msg_id) = wire::peek(body).ok_or("not one whole message")?;
        if [id::PHONE_SETUP_FAILED, id::PHONE_FAILED, id::PHONE_GAVE_UP].contains(&msg_id) {
            self.step = Step::Ended;
            return Err(format!("the phone gave up ({msg_id:#06x})"));
        }
        let msg = if start == CLEAR {
            wire::decode(body)?
        } else if self.session_id != 0 && start == self.session_id {
            match self.open(msg_id, body) {
                Some(msg) => msg,
                None => {
                    println!("[cp] mfi4: {msg_id:#06x} does not open");
                    return Ok(clear(id::RESET));
                }
            }
        } else {
            println!("[cp] mfi4: {msg_id:#06x} under start {start:#06x}");
            return Ok(clear(id::FAILED));
        };

        let answer = match (msg_id, self.step) {
            (id::PHONE_SETUP, Step::Fresh) => {
                if chip.protocol_major().await? != OUR_VERSION {
                    return Err("mutual authentication needs a 3.0 coprocessor".into());
                }
                return Ok(self.setup(&msg).unwrap_or_else(|e| {
                    println!("[cp] mfi4: setup refused: {e}");
                    clear(id::SETUP_FAILED)
                }));
            }
            (id::PHONE_START, Step::SetUp) => self.start(&msg, chip).await,
            (id::PHONE_CERT, Step::AwaitingPhoneCert) => self.phone_cert(&msg, chip).await,
            (id::PHONE_WANTS_CERT, Step::Finished) => self.our_cert(&msg),
            (id::PHONE_SUCCEEDED, Step::Finished) => {
                println!("[cp] mfi4: the phone and LIVI proved each other");
                self.step = Step::Proven;
                Ok(Message::new(self.session_id, id::SUCCEEDED))
            }
            (id::PHONE_TUNNEL_DATA, Step::Proven) => {
                return Ok(self.tunnel_in(&msg, tunnel));
            }
            (id::PHONE_TUNNEL_READ, Step::Proven) => {
                let out = std::mem::take(&mut self.tunnel_out);
                let inner = Message::new(TUNNELED, id::TUNNELED_FROM_US)
                    .with(0, [PAIRING_DATA])
                    .with(1, out)
                    .encode();
                Ok(Message::new(self.session_id, id::TUNNEL_DATA).with(0, inner))
            }
            (other, step) => Err(Fault::Malformed(format!("{other:#06x} while {step:?}"))),
        };
        Ok(match answer {
            Ok(msg) => self.seal(&msg),
            Err(Fault::Malformed(e)) => {
                println!("[cp] mfi4: {e}");
                clear(id::FAILED)
            }
            Err(Fault::Operation(e)) => {
                println!("[cp] mfi4: {e}");
                clear(id::GAVE_UP)
            }
        })
    }

    fn setup(&mut self, msg: &Message) -> Result<Vec<u8>, String> {
        let lowest = msg.u8(0).ok_or("no lowest version")?;
        let highest = msg.u8(1).ok_or("no highest version")?;
        if lowest < LOWEST_VERSION {
            return Err(format!("the phone starts at version {lowest}"));
        }
        if msg.u8(2) != Some(PHONE_PROVES_AS_DEVICE) {
            return Err(format!("certificate chain {:?}", msg.u8(2)));
        }
        let auth_types = single_bytes(msg, 3)?;
        let capabilities = single_bytes(msg, 4)?;
        if auth_types.is_empty()
            || auth_types.len() > MAX_AUTH_TYPES
            || capabilities.len() > MAX_CAPABILITIES
        {
            return Err(format!(
                "{} kinds, {} capabilities",
                auth_types.len(),
                capabilities.len()
            ));
        }
        if !auth_types.contains(&WE_RESPOND) {
            return Err(format!("the phone offers {auth_types:?}, we only respond"));
        }
        self.version = highest.min(OUR_VERSION);
        self.phone_auth_types = auth_types;
        self.phone_capabilities = capabilities;
        self.our_nonce = loop {
            let n = u16::from_be_bytes(random_bytes::<2>());
            if n != 0 && n != u16::MAX {
                break n;
            }
        };
        let ephemeral = Ephemeral::generate()?;
        let answer = Message::new(CLEAR, id::SETUP)
            .with(0, [self.version])
            .with(1, [WE_RESPOND])
            .with(2, [OUR_CHAIN_IS_ACCESSORY])
            .with(3, self.our_nonce.to_be_bytes())
            .with(4, ephemeral.public);
        self.ephemeral = Some(ephemeral);
        self.step = Step::SetUp;
        Ok(answer.encode())
    }

    async fn start(&mut self, msg: &Message, chip: &impl MfiSigner) -> Result<Message, Fault> {
        let malformed = |what: &str| Fault::Malformed(format!("AuthStart: {what}"));
        let nonce = msg.u16(0).ok_or_else(|| malformed("no nonce"))?;
        let point = msg
            .exact::<POINT>(1)
            .ok_or_else(|| malformed("no 33 byte key"))?;
        let sealed = msg
            .exact::<{ HASH + SIGNATURE + TAG }>(2)
            .ok_or_else(|| malformed("seal"))?;
        if nonce == 0 || nonce == u16::MAX {
            return Err(malformed("a reserved nonce"));
        }
        self.session_id = nonce ^ self.our_nonce;
        if self.session_id == 0 {
            return Err(malformed("both nonces are the same"));
        }
        let ephemeral = self.ephemeral.as_ref().expect("set up before");
        let shared = ephemeral.shared(&point).map_err(Fault::Malformed)?;
        let negotiation = [
            &[OUR_VERSION][..],
            &self.phone_auth_types,
            &self.phone_capabilities,
            &[self.version, WE_RESPOND],
        ]
        .concat();
        let context = [&[keys::SIGMA][..], &ephemeral.public, &point, &negotiation].concat();
        let mut keys = keys::derive(&shared, &context);
        let opened = keys
            .start
            .open(&[], &sealed)
            .ok_or_else(|| malformed("the seal does not open"))?;
        self.phone_hash.copy_from_slice(&opened[..HASH]);
        self.phone_signature.copy_from_slice(&opened[HASH..]);
        self.phone_point = point;
        self.keys = Some(keys);
        self.our_prefix = random_bytes::<PREFIX>();
        self.our_cert = chip.certificate().await.map_err(Fault::Operation)?;
        self.our_hash = sha256(&[&self.our_prefix, &self.our_cert]);
        self.step = Step::AwaitingPhoneCert;
        Ok(Message::new(self.session_id, id::WANT_CERT).with(0, self.phone_hash))
    }

    async fn phone_cert(&mut self, msg: &Message, chip: &impl MfiSigner) -> Result<Message, Fault> {
        let malformed = |what: String| Fault::Malformed(format!("phone certificate: {what}"));
        let kind = msg.u8(0).ok_or_else(|| malformed("no kind".into()))?;
        let packed = msg.param(1).ok_or_else(|| malformed("none".into()))?;
        let mut ids = msg.ids();
        ids.sort_unstable();
        let unpacked = cert::unpack(packed).map_err(malformed)?;
        if kind == PHONE_LEAF {
            if ids != [0, 1, 2] {
                return Err(malformed(format!("leaf parameters {ids:?}")));
            }
            let prefix = msg.param(2).expect("checked");
            if sha256(&[prefix, &unpacked.der]) != self.phone_hash {
                return Err(malformed("it is not the one the phone signed for".into()));
            }
            if self.trust.rooted() && !self.trust.knows_issuer_of(&unpacked) {
                let issuer = unpacked.authority;
                self.leaf_waiting = Some(unpacked);
                return Ok(Message::new(self.session_id, id::WANT_CERT).with(1, issuer));
            }
            return self.finish(unpacked, chip).await;
        }
        if !PHONE_INTERMEDIATE.contains(&kind) || ids != [0, 1] {
            return Err(malformed(format!("kind {kind} with {ids:?}")));
        }
        self.trust.admit(&unpacked).map_err(malformed)?;
        let leaf = self
            .leaf_waiting
            .take()
            .ok_or_else(|| malformed("no leaf before".into()))?;
        self.finish(leaf, chip).await
    }

    async fn finish(&mut self, leaf: Unpacked, chip: &impl MfiSigner) -> Result<Message, Fault> {
        self.trust
            .check(&leaf)
            .map_err(|e| Fault::Malformed(format!("phone leaf: {e}")))?;
        let keys = self.keys.as_ref().expect("derived at start");
        let ours = self.ephemeral.as_ref().expect("set up before").public;
        let phone_mac = keys::cmac(&keys.phone_mac, &self.phone_hash);
        let theirs = keys::signed_data(&ours, &self.phone_point, &phone_mac);
        if !cert::signed_by(&leaf, &theirs, &self.phone_signature).map_err(Fault::Malformed)? {
            return Err(Fault::Malformed(
                "the phone's signature does not verify".into(),
            ));
        }
        let our_mac = keys::cmac(&keys.our_mac, &self.our_hash);
        let digest = sha256(&[&keys::signed_data(&ours, &self.phone_point, &our_mac)]);
        let signature = chip.sign(&digest).await.map_err(Fault::Operation)?;
        if signature.len() != SIGNATURE {
            return Err(Fault::Operation(format!(
                "a {} byte signature",
                signature.len()
            )));
        }
        self.step = Step::Finished;
        Ok(Message::new(self.session_id, id::FINISH)
            .with(0, self.our_hash)
            .with(1, signature))
    }

    fn our_cert(&mut self, msg: &Message) -> Result<Message, Fault> {
        match (msg.param(0), msg.param(1)) {
            (Some(hash), None) if hash == self.our_hash => {
                Ok(Message::new(self.session_id, id::CERT)
                    .with(0, [OUR_CERT_IS_3_0_LEAF])
                    .with(1, self.our_cert.clone())
                    .with(2, self.our_prefix))
            }
            (Some(_), None) => Err(Fault::Operation("the phone asks for another hash".into())),
            (None, Some(_)) => Err(Fault::Operation(
                "the phone asks for an intermediate".into(),
            )),
            _ => Err(Fault::Malformed(
                "certificate request without hash or key id".into(),
            )),
        }
    }

    fn tunnel_in(&mut self, msg: &Message, tunnel: Tunnel<'_>) -> Vec<u8> {
        let result = (|| {
            if !self.binds_pairing {
                return Err("the phone did not say it binds the pairing".to_string());
            }
            let (_, outer) = msg.params.first().ok_or("an empty tunnel message")?;
            let inner = wire::decode(outer)?;
            if inner.start != TUNNELED
                || ![id::TUNNELED_FROM_PHONE, id::TUNNELED_FROM_US].contains(&inner.id)
            {
                return Err(format!(
                    "tunneled {:#06x} under {:#06x}",
                    inner.id, inner.start
                ));
            }
            tunnel(inner.param(1).ok_or("tunneled message without data")?)
        })();
        match result {
            Ok(out) => {
                self.tunnel_out = out;
                self.seal(&Message::new(self.session_id, id::TUNNEL_TAKEN))
            }
            Err(e) => {
                println!("[cp] mfi4: tunnel: {e}");
                Message::new(self.session_id, id::TUNNEL_FAILED).encode()
            }
        }
    }

    fn channel(&mut self, msg_id: u16, ours: bool) -> Option<&mut Channel> {
        let keys = self.keys.as_mut()?;
        Some(match (msg_id, ours) {
            (id::WANT_CERT | id::PHONE_CERT, _) => &mut keys.phone_cert,
            (id::CERT | id::PHONE_WANTS_CERT, _) => &mut keys.our_cert,
            (id::FINISH, _) => &mut keys.finish,
            (_, true) => &mut keys.ours,
            (_, false) => &mut keys.from_phone,
        })
    }

    fn open(&mut self, msg_id: u16, body: &[u8]) -> Option<Message> {
        let channel = self.channel(msg_id, false)?;
        let plain = channel.open(&body[..HEADER], &body[HEADER..])?;
        let params = wire::decode_params(&plain).ok()?;
        let (start, msg_id) = wire::peek(body)?;
        Some(Message {
            start,
            id: msg_id,
            params,
        })
    }

    fn seal(&mut self, msg: &Message) -> Vec<u8> {
        let params = msg.encode_params();
        let header = msg.header(params.len() + TAG);
        let channel = self
            .channel(msg.id, true)
            .expect("keys before anything is sealed");
        [&header[..], &channel.seal(&header, &params)].concat()
    }
}

fn clear(msg_id: u16) -> Vec<u8> {
    Message::new(CLEAR, msg_id).encode()
}

fn single_bytes(msg: &Message, param: u16) -> Result<Vec<u8>, String> {
    msg.all(param)
        .map(|v| match v {
            [b] => Ok(*b),
            _ => Err(format!("parameter {param} is {} bytes", v.len())),
        })
        .collect()
}
