// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::net::Ipv6Addr;

use livi_session_io::usb::UsbError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no local USB backend on this platform")]
    Unsupported,
    #[error("iphone {0} not found")]
    NotFound(String),
    #[error("{0} is not on the bus")]
    NotOnBus(String),
    #[error("iphone gone after {0}")]
    Gone(&'static str),
    #[error("iphone {0} did not expose CarPlay configs")]
    NoCarplayConfigs(String),
    #[error("usb device {bus}/{address} not found")]
    NoUsbDevice { bus: u8, address: u8 },
    #[error("no NCM function in the active config")]
    NoNcmFunction,
    #[error("no interface carries {0}")]
    NoInterface(Ipv6Addr),
    #[error("mux connect to port {0} failed")]
    ConnectFailed(u16),
    #[error("{what}: {source}")]
    Io { what: String, source: io::Error },
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[error(transparent)]
    ListDevices(nusb::Error),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[error("{what}: {source}")]
    Usb { what: String, source: nusb::Error },
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[error("{what}: {source}")]
    Transfer {
        what: String,
        source: nusb::transfer::TransferError,
    },
    #[error("usbmux interface: {0}")]
    UsbmuxInterface(#[from] UsbError),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    #[error("{0}")]
    OpenTun(#[source] io::Error),
}

impl Error {
    pub(crate) fn io(what: impl Into<String>) -> impl FnOnce(io::Error) -> Self {
        move |source| Self::Io {
            what: what.into(),
            source,
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn usb(what: impl Into<String>) -> impl FnOnce(nusb::Error) -> Self {
        move |source| Self::Usb {
            what: what.into(),
            source,
        }
    }
}
