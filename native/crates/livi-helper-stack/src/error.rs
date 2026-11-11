// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error(transparent)]
    Chip(#[from] iap2_mfi::MfiError),
    #[error(transparent)]
    Task(#[from] tokio::task::JoinError),
}

#[derive(Debug, thiserror::Error)]
pub enum BtError {
    #[error(transparent)]
    Bus(#[from] zbus::Error),
    #[error("BlueZ never published {path}: {source}")]
    NoAdapter { path: String, source: zbus::Error },
    #[error("{0} requires a MAC")]
    NoMac(&'static str),
    #[error("{0}")]
    Other(String),
}

impl From<String> for BtError {
    fn from(message: String) -> Self {
        BtError::Other(message)
    }
}
