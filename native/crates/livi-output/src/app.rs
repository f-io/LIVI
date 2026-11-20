// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--probe") {
        println!("{}", crate::gst::probe());
        return;
    }

    crate::process::run(
        args.first().map_or("", String::as_str),
        args.get(1).map_or("", String::as_str),
    );
}
