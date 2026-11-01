// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::path::{Path, PathBuf};

use livi_ui_model::i18n::Strings;
use livi_ui_model::settings::custom::files::{Entry, Wanted, entry_rows, listing};
use livi_ui_model::settings::view::Row;

const VOLUMES: [&str; 4] = ["/Volumes", "/media", "/run/media", "/mnt"];

pub struct Browser {
    wanted: Wanted,
    roots: Vec<PathBuf>,
    dir: Option<PathBuf>,
    entries: Vec<(Entry, PathBuf)>,
}

pub enum Picked {
    Entered,
    File(Wanted, PathBuf),
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn read_dir(dir: &Path, wanted: Wanted) -> Vec<(Entry, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let raw: Vec<(Entry, PathBuf)> = read
        .flatten()
        .map(|e| {
            let path = e.path();
            let entry = Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                dir: path.is_dir(),
            };
            (entry, path)
        })
        .collect();
    let kept = listing(raw.iter().map(|(e, _)| e.clone()).collect(), wanted);
    kept.into_iter()
        .filter_map(|e| raw.iter().find(|(r, _)| *r == e).cloned())
        .collect()
}

impl Browser {
    pub fn open(wanted: Wanted) -> Self {
        let roots: Vec<PathBuf> = home()
            .into_iter()
            .chain(VOLUMES.iter().map(PathBuf::from))
            .filter(|p| p.is_dir())
            .collect();
        Self::within(wanted, roots)
    }

    fn within(wanted: Wanted, roots: Vec<PathBuf>) -> Self {
        let mut b = Self {
            wanted,
            roots,
            dir: None,
            entries: Vec::new(),
        };
        b.show(None);
        b
    }

    fn show(&mut self, dir: Option<PathBuf>) {
        self.entries = match &dir {
            Some(d) => read_dir(d, self.wanted),
            None => self
                .roots
                .iter()
                .map(|p| {
                    (
                        Entry {
                            name: name_of(p),
                            dir: true,
                        },
                        p.clone(),
                    )
                })
                .collect(),
        };
        self.dir = dir;
    }

    pub fn depth(&self) -> usize {
        let Some(dir) = &self.dir else { return 1 };
        let root = self.roots.iter().find(|r| dir.starts_with(r));
        let below = root.map_or(0, |r| dir.components().count() - r.components().count());
        2 + below
    }

    pub fn pick(&mut self, i: usize) -> Option<Picked> {
        let (entry, path) = self.entries.get(i).cloned()?;
        if entry.dir {
            self.show(Some(path));
            Some(Picked::Entered)
        } else {
            Some(Picked::File(self.wanted, path))
        }
    }

    pub fn up(&mut self) -> bool {
        let Some(dir) = self.dir.clone() else {
            return false;
        };
        let parent = (!self.roots.contains(&dir))
            .then(|| dir.parent().map(Path::to_path_buf))
            .flatten();
        self.show(parent);
        true
    }

    pub fn title(&self, t: &Strings) -> String {
        match &self.dir {
            Some(d) => name_of(d),
            None => self.wanted.title(t),
        }
    }

    pub fn rows(&self, t: &Strings) -> Vec<Row> {
        let entries: Vec<Entry> = self.entries.iter().map(|(e, _)| e.clone()).collect();
        entry_rows(&entries, self.wanted, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_walks_down_and_back_up() {
        let root = std::env::temp_dir().join(format!("livi-files-test-{}", std::process::id()));
        let inner = root.join("keys");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("identity.pk8"), b"x").unwrap();
        std::fs::write(inner.join("a.png"), b"x").unwrap();
        let mut b = Browser::within(Wanted::Identity, vec![root.clone()]);
        assert_eq!(b.depth(), 1);
        assert!(matches!(b.pick(0), Some(Picked::Entered)));
        assert_eq!(b.depth(), 2);
        assert!(matches!(b.pick(0), Some(Picked::Entered)));
        assert_eq!(b.depth(), 3);
        assert_eq!(b.entries.len(), 1);
        assert!(matches!(
            b.pick(0),
            Some(Picked::File(Wanted::Identity, p)) if p == inner.join("identity.pk8")
        ));
        assert!(b.up());
        assert_eq!(b.dir.as_deref(), Some(root.as_path()));
        assert!(b.up());
        assert_eq!(b.dir, None);
        assert!(!b.up());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
