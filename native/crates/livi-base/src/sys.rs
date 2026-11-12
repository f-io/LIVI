// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

pub fn tool(name: &str) -> String {
    for dir in ["/usr/sbin", "/sbin", "/usr/bin", "/bin"] {
        let path = format!("{dir}/{name}");
        if std::path::Path::new(&path).exists() {
            return path;
        }
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::tool;

    #[test]
    fn a_program_that_is_there_comes_back_with_its_directory() {
        let sh = tool("sh");
        assert!(sh.starts_with('/') && sh.ends_with("/sh"), "{sh}");
    }

    #[test]
    fn an_unknown_program_stays_as_it_was() {
        assert_eq!(tool("livi-no-such-program"), "livi-no-such-program");
    }
}
