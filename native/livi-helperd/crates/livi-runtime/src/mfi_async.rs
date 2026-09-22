use std::sync::{Arc, Mutex};

use iap2_mfi::AuthCoprocessor;

use crate::AsyncAuth;

/// Backend-agnostic shared MFi coprocessor: local i2c (`I2cCoprocessor`) or the
/// remote LIVI Link dongle over NCM (`NcmCoprocessor`) — both implement `AuthCoprocessor`.
#[derive(Clone)]
pub struct SharedCoprocessor {
    inner: Arc<Mutex<Box<dyn AuthCoprocessor + Send>>>,
}

impl SharedCoprocessor {
    pub fn new(chip: Box<dyn AuthCoprocessor + Send>) -> Self {
        Self { inner: Arc::new(Mutex::new(chip)) }
    }

    /// Puts another chip behind every clone of this handle.
    pub fn replace(&self, chip: Box<dyn AuthCoprocessor + Send>) {
        *self.inner.lock().unwrap() = chip;
    }
}

impl AsyncAuth for SharedCoprocessor {
    async fn read_certificate(&mut self) -> Result<Vec<u8>, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            inner.lock().unwrap().read_certificate().map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn sign(&mut self, challenge: Vec<u8>) -> Result<Vec<u8>, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            inner.lock().unwrap().generate_challenge_response(&challenge).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn protocol_major(&mut self) -> Result<u8, String> {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            inner.lock().unwrap().protocol_major().map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// AsyncAuth that always errors (no MFi chip).
#[derive(Clone)]
pub struct NoAuth;

impl AsyncAuth for NoAuth {
    async fn read_certificate(&mut self) -> Result<Vec<u8>, String> {
        Err("no MFi coprocessor".into())
    }

    async fn sign(&mut self, _challenge: Vec<u8>) -> Result<Vec<u8>, String> {
        Err("no MFi coprocessor".into())
    }

    async fn protocol_major(&mut self) -> Result<u8, String> {
        Err("no MFi coprocessor".into())
    }
}
