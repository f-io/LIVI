// iAP2 control session message codec: 0x4040-framed messages with length-prefixed params.

pub mod messages;

pub const CSM_START: u16 = 0x4040;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Header,
    WrongMsgId { expected: u16, got: u16 },
    MissingParam { message: &'static str, param: &'static str },
    Scalar { param: &'static str },
    Enum { param: &'static str, value: u8 },
    Utf8 { param: &'static str },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Header => write!(f, "bad csm header"),
            Error::WrongMsgId { expected, got } => {
                write!(f, "msg id 0x{got:04X}, expected 0x{expected:04X}")
            }
            Error::MissingParam { message, param } => {
                write!(f, "{message}: required param {param} missing")
            }
            Error::Scalar { param } => write!(f, "bad scalar payload for {param}"),
            Error::Enum { param, value } => write!(f, "unknown enum value {value} for {param}"),
            Error::Utf8 { param } => write!(f, "invalid utf-8 in {param}"),
        }
    }
}

impl std::error::Error for Error {}

pub fn put_param(out: &mut Vec<u8>, id: u16, payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u16 + 4).to_be_bytes());
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(payload);
}

pub fn split_params(mut buf: &[u8]) -> Vec<(u16, &[u8])> {
    let mut params = Vec::new();
    while buf.len() >= 4 {
        let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        let id = u16::from_be_bytes([buf[2], buf[3]]);
        if len < 4 || len > buf.len() {
            break;
        }
        params.push((id, &buf[4..len]));
        buf = &buf[len..];
    }
    params
}

pub fn frame_header(frame: &[u8]) -> Result<(u16, &[u8]), Error> {
    if frame.len() < 6 {
        return Err(Error::Header);
    }
    let start = u16::from_be_bytes([frame[0], frame[1]]);
    let len = u16::from_be_bytes([frame[2], frame[3]]) as usize;
    let msg_id = u16::from_be_bytes([frame[4], frame[5]]);
    if start != CSM_START || len < 6 || len > frame.len() {
        return Err(Error::Header);
    }
    Ok((msg_id, &frame[6..len]))
}

pub trait CsmParams: Sized {
    fn encode_params(&self, out: &mut Vec<u8>);
    fn decode_params(params: &[(u16, &[u8])]) -> Result<Self, Error>;
}

pub trait CsmMessage: CsmParams {
    const MSG_ID: u16;

    fn encode(&self) -> Vec<u8> {
        let mut params = Vec::new();
        self.encode_params(&mut params);
        let mut out = Vec::with_capacity(params.len() + 6);
        out.extend_from_slice(&CSM_START.to_be_bytes());
        out.extend_from_slice(&(params.len() as u16 + 6).to_be_bytes());
        out.extend_from_slice(&Self::MSG_ID.to_be_bytes());
        out.extend_from_slice(&params);
        out
    }

    fn decode(frame: &[u8]) -> Result<Self, Error> {
        let (msg_id, payload) = frame_header(frame)?;
        if msg_id != Self::MSG_ID {
            return Err(Error::WrongMsgId { expected: Self::MSG_ID, got: msg_id });
        }
        Self::decode_params(&split_params(payload))
    }
}

#[macro_export]
macro_rules! csm_enum {
    ($(#[$m:meta])* $vis:vis enum $name:ident { $($var:ident = $val:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        $vis enum $name { $($var = $val),+ }

        impl $name {
            pub fn from_u8(v: u8) -> Option<Self> {
                match v { $($val => Some(Self::$var),)+ _ => None }
            }
        }
    };
}

#[macro_export]
macro_rules! csm_field_ty {
    (opt $($k:tt)+) => { Option<$crate::csm_field_ty!($($k)+)> };
    (list $($k:tt)+) => { Vec<$crate::csm_field_ty!($($k)+)> };
    (flag) => { bool };
    (bool) => { bool };
    (i8) => { i8 };
    (u8) => { u8 };
    (i16) => { i16 };
    (u16) => { u16 };
    (i32) => { i32 };
    (u32) => { u32 };
    (i64) => { i64 };
    (u64) => { u64 };
    (str) => { String };
    (bytes) => { Vec<u8> };
    (enum $e:ty) => { $e };
    (group $g:ty) => { $g };
}

#[macro_export]
macro_rules! csm_val_encode {
    ($v:expr_2021, bool) => { vec![*$v as u8] };
    ($v:expr_2021, i8) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, u8) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, i16) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, u16) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, i32) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, u32) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, i64) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, u64) => { $v.to_be_bytes().to_vec() };
    ($v:expr_2021, str) => {{
        let mut p = $v.as_bytes().to_vec();
        p.push(0);
        p
    }};
    ($v:expr_2021, bytes) => { $v.clone() };
    ($v:expr_2021, enum $e:ty) => { vec![*$v as u8] };
    ($v:expr_2021, group $g:ty) => {{
        let mut p = Vec::new();
        $crate::CsmParams::encode_params($v, &mut p);
        p
    }};
}

#[macro_export]
macro_rules! csm_val_decode {
    ($b:expr_2021, $n:expr_2021, bool) => {
        match $b {
            [v] => Ok::<_, $crate::Error>(*v != 0),
            _ => Err($crate::Error::Scalar { param: $n }),
        }
    };
    ($b:expr_2021, $n:expr_2021, i8) => { $crate::csm_int_decode!($b, $n, i8, 1) };
    ($b:expr_2021, $n:expr_2021, u8) => { $crate::csm_int_decode!($b, $n, u8, 1) };
    ($b:expr_2021, $n:expr_2021, i16) => { $crate::csm_int_decode!($b, $n, i16, 2) };
    ($b:expr_2021, $n:expr_2021, u16) => { $crate::csm_int_decode!($b, $n, u16, 2) };
    ($b:expr_2021, $n:expr_2021, i32) => { $crate::csm_int_decode!($b, $n, i32, 4) };
    ($b:expr_2021, $n:expr_2021, u32) => { $crate::csm_int_decode!($b, $n, u32, 4) };
    ($b:expr_2021, $n:expr_2021, i64) => { $crate::csm_int_decode!($b, $n, i64, 8) };
    ($b:expr_2021, $n:expr_2021, u64) => { $crate::csm_int_decode!($b, $n, u64, 8) };
    ($b:expr_2021, $n:expr_2021, str) => {{
        let b: &[u8] = $b;
        let cut = if b.is_empty() { b } else { &b[..b.len() - 1] };
        core::str::from_utf8(cut)
            .map(str::to_owned)
            .map_err(|_| $crate::Error::Utf8 { param: $n })
    }};
    ($b:expr_2021, $n:expr_2021, bytes) => { Ok::<_, $crate::Error>($b.to_vec()) };
    ($b:expr_2021, $n:expr_2021, enum $e:ty) => {
        match $b.first() {
            Some(&v) => <$e>::from_u8(v).ok_or($crate::Error::Enum { param: $n, value: v }),
            None => Err($crate::Error::Scalar { param: $n }),
        }
    };
    ($b:expr_2021, $n:expr_2021, group $g:ty) => {
        <$g as $crate::CsmParams>::decode_params(&$crate::split_params($b))
    };
}

#[macro_export]
macro_rules! csm_int_decode {
    ($b:expr_2021, $n:expr_2021, $t:ty, $len:literal) => {
        <[u8; $len]>::try_from($b)
            .map(<$t>::from_be_bytes)
            .map_err(|_| $crate::Error::Scalar { param: $n })
    };
}

#[macro_export]
macro_rules! csm_opt_decode {
    ($b:expr_2021, $n:expr_2021, bool) => { $crate::csm_opt_scalar_decode!($b, $n, bool) };
    ($b:expr_2021, $n:expr_2021, i8) => { $crate::csm_opt_scalar_decode!($b, $n, i8) };
    ($b:expr_2021, $n:expr_2021, u8) => { $crate::csm_opt_scalar_decode!($b, $n, u8) };
    ($b:expr_2021, $n:expr_2021, i16) => { $crate::csm_opt_scalar_decode!($b, $n, i16) };
    ($b:expr_2021, $n:expr_2021, u16) => { $crate::csm_opt_scalar_decode!($b, $n, u16) };
    ($b:expr_2021, $n:expr_2021, i32) => { $crate::csm_opt_scalar_decode!($b, $n, i32) };
    ($b:expr_2021, $n:expr_2021, u32) => { $crate::csm_opt_scalar_decode!($b, $n, u32) };
    ($b:expr_2021, $n:expr_2021, i64) => { $crate::csm_opt_scalar_decode!($b, $n, i64) };
    ($b:expr_2021, $n:expr_2021, u64) => { $crate::csm_opt_scalar_decode!($b, $n, u64) };
    ($b:expr_2021, $n:expr_2021, $($k:tt)+) => {
        $crate::csm_val_decode!($b, $n, $($k)+).map(Some)
    };
}

#[macro_export]
macro_rules! csm_opt_scalar_decode {
    ($b:expr_2021, $n:expr_2021, $k:tt) => {
        if $b.is_empty() {
            Ok(None)
        } else {
            $crate::csm_val_decode!($b, $n, $k).map(Some)
        }
    };
}

#[macro_export]
macro_rules! csm_field_encode {
    ($out:expr_2021, $pid:expr_2021, $v:expr_2021, opt $($k:tt)+) => {
        if let Some(v) = &$v {
            $crate::put_param($out, $pid, &$crate::csm_val_encode!(v, $($k)+));
        }
    };
    // Lists go on the wire as one param with all element payloads concatenated.
    ($out:expr_2021, $pid:expr_2021, $v:expr_2021, list $($k:tt)+) => {
        if !$v.is_empty() {
            let mut p = Vec::new();
            for v in &$v {
                p.extend_from_slice(&$crate::csm_val_encode!(v, $($k)+));
            }
            $crate::put_param($out, $pid, &p);
        }
    };
    ($out:expr_2021, $pid:expr_2021, $v:expr_2021, flag) => {
        if $v {
            $crate::put_param($out, $pid, &[]);
        }
    };
    ($out:expr_2021, $pid:expr_2021, $v:expr_2021, $($k:tt)+) => {{
        let v = &$v;
        $crate::put_param($out, $pid, &$crate::csm_val_encode!(v, $($k)+));
    }};
}

#[macro_export]
macro_rules! csm_field_decode {
    ($params:expr_2021, $pid:expr_2021, $msg:expr_2021, $n:expr_2021, opt $($k:tt)+) => {
        match $params.iter().find(|(id, _)| *id == $pid) {
            None => Ok(None),
            Some((_, b)) => $crate::csm_opt_decode!(*b, $n, $($k)+),
        }
    };
    ($params:expr_2021, $pid:expr_2021, $msg:expr_2021, $n:expr_2021, list $($k:tt)+) => {
        match $params.iter().find(|(id, _)| *id == $pid) {
            None => Ok(Vec::new()),
            Some((_, b)) => $crate::csm_val_decode!(*b, $n, $($k)+).map(|v| vec![v]),
        }
    };
    ($params:expr_2021, $pid:expr_2021, $msg:expr_2021, $n:expr_2021, flag) => {
        Ok::<_, $crate::Error>($params.iter().any(|(id, _)| *id == $pid))
    };
    ($params:expr_2021, $pid:expr_2021, $msg:expr_2021, $n:expr_2021, $($k:tt)+) => {
        match $params.iter().find(|(id, _)| *id == $pid) {
            Some((_, b)) => $crate::csm_val_decode!(*b, $n, $($k)+),
            None => Err($crate::Error::MissingParam { message: $msg, param: $n }),
        }
    };
}

#[macro_export]
macro_rules! csm_group {
    (
        $(#[$m:meta])*
        $vis:vis struct $name:ident {
            $( $pid:literal => $f:ident: [$($k:tt)+] ),* $(,)?
        }
    ) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq)]
        $vis struct $name {
            $( pub $f: $crate::csm_field_ty!($($k)+), )*
        }

        impl $crate::CsmParams for $name {
            fn encode_params(&self, out: &mut Vec<u8>) {
                $( $crate::csm_field_encode!(out, $pid, self.$f, $($k)+); )*
                let _ = out;
            }

            fn decode_params(params: &[(u16, &[u8])]) -> Result<Self, $crate::Error> {
                let _ = params;
                Ok(Self {
                    $( $f: $crate::csm_field_decode!(
                        params, $pid, stringify!($name), stringify!($f), $($k)+)?, )*
                })
            }
        }
    };
}

#[macro_export]
macro_rules! csm_message {
    (
        $(#[$m:meta])*
        $vis:vis struct $name:ident = $id:literal {
            $($body:tt)*
        }
    ) => {
        $crate::csm_group! {
            $(#[$m])*
            $vis struct $name { $($body)* }
        }

        impl $crate::CsmMessage for $name {
            const MSG_ID: u16 = $id;
        }
    };
}
