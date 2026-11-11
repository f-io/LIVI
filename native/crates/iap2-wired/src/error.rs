// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::io;
use std::path::PathBuf;

use idevice::IdeviceError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Mux(#[from] iap2_usbmux::Error),
    #[error(transparent)]
    Channel(IdeviceError),
    #[error("{what}: {source}")]
    Lockdown { what: String, source: IdeviceError },
    #[error("{0}")]
    Store(#[source] io::Error),
    #[error("write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
}

impl Error {
    pub(crate) fn lockdown(what: impl Into<String>) -> impl FnOnce(IdeviceError) -> Self {
        move |source| Self::Lockdown {
            what: what.into(),
            source,
        }
    }
}
