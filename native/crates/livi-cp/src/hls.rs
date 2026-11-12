// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use openssl::ssl::{SslConnector, SslMethod};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_openssl::SslStream;

use crate::video_proxy::ProxyOffer;

mod http;
mod playlist;

pub use http::*;
pub use playlist::*;
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(4);

#[cfg(test)]
mod tests;
