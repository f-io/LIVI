// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::Path;

use livi_core::InProcessOutput;

pub struct InProcess;

impl InProcessOutput for InProcess {
    fn serve(&self, sock: &Path) {
        livi_output::process::serve(sock);
    }

    fn probe(&self) -> livi_output_proto::Probe {
        livi_output::gst::probe()
    }
}
