// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let builder = builder.tls_certs_only(android::roots().iter().cloned());
    builder
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::OnceLock;

    use reqwest::Certificate;

    pub fn roots() -> &'static [Certificate] {
        static ROOTS: OnceLock<Vec<Certificate>> = OnceLock::new();
        ROOTS.get_or_init(|| {
            let roots: Vec<Certificate> = livi_net::ca::android_roots()
                .iter()
                .filter_map(|pem| Certificate::from_pem(pem).ok())
                .collect();
            if roots.is_empty() {
                eprintln!("[http] no root certificates on this device");
            }
            roots
        })
    }
}
