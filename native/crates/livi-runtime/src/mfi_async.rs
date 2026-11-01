// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::{Arc, Mutex};

use iap2_mfi::AuthCoprocessor;

use crate::AsyncAuth;

#[derive(Clone)]
pub struct SharedCoprocessor {
    inner: Arc<Mutex<Box<dyn AuthCoprocessor + Send>>>,
}

impl SharedCoprocessor {
    pub fn new(chip: Box<dyn AuthCoprocessor + Send>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(chip)),
        }
    }

    pub fn replace(&self, chip: Box<dyn AuthCoprocessor + Send>) {
        *self.inner.lock().unwrap() = chip;
    }
}

impl AsyncAuth for SharedCoprocessor {
    async fn read_certificate(&mut self) -> Result<Vec<u8>, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            inner
                .lock()
                .unwrap()
                .read_certificate()
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn sign(&mut self, challenge: Vec<u8>) -> Result<Vec<u8>, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            inner
                .lock()
                .unwrap()
                .generate_challenge_response(&challenge)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn protocol_major(&mut self) -> Result<u8, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            inner
                .lock()
                .unwrap()
                .protocol_major()
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
