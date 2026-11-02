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

    const SYSTEM_ROOTS: &str = "/apex/com.android.conscrypt/cacerts";

    pub fn roots() -> &'static [Certificate] {
        static ROOTS: OnceLock<Vec<Certificate>> = OnceLock::new();
        ROOTS.get_or_init(|| {
            let roots: Vec<Certificate> = std::fs::read_dir(SYSTEM_ROOTS)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|entry| std::fs::read(entry.path()).ok())
                .filter_map(|pem| Certificate::from_pem(&pem).ok())
                .collect();
            if roots.is_empty() {
                eprintln!("[http] no root certificates in {SYSTEM_ROOTS}");
            }
            roots
        })
    }
}
