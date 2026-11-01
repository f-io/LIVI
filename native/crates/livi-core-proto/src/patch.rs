// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum PatchOp {
    Set { path: Vec<String>, value: Value },
    Remove { path: Vec<String> },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PatchError {
    MissingParent(Vec<String>),
    MissingKey(Vec<String>),
    RemoveRoot,
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingParent(path) => write!(f, "no object to set {}", path.join(".")),
            Self::MissingKey(path) => write!(f, "nothing to remove at {}", path.join(".")),
            Self::RemoveRoot => write!(f, "the root cannot be removed"),
        }
    }
}

impl std::error::Error for PatchError {}

pub fn diff(old: &Value, new: &Value) -> Vec<PatchOp> {
    let mut ops = Vec::new();
    diff_into(old, new, &mut Vec::new(), &mut ops);
    ops
}

fn diff_into(old: &Value, new: &Value, path: &mut Vec<String>, ops: &mut Vec<PatchOp>) {
    match (old, new) {
        (Value::Object(before), Value::Object(after)) => {
            for (key, value) in after {
                path.push(key.clone());
                match before.get(key) {
                    Some(prev) => diff_into(prev, value, path, ops),
                    None => ops.push(PatchOp::Set {
                        path: path.clone(),
                        value: value.clone(),
                    }),
                }
                path.pop();
            }
            for key in before.keys().filter(|k| !after.contains_key(*k)) {
                let mut gone = path.clone();
                gone.push(key.clone());
                ops.push(PatchOp::Remove { path: gone });
            }
        }
        _ if old != new => ops.push(PatchOp::Set {
            path: path.clone(),
            value: new.clone(),
        }),
        _ => {}
    }
}

pub fn apply(target: &mut Value, op: &PatchOp) -> Result<(), PatchError> {
    match op {
        PatchOp::Set { path, value } => {
            let Some((key, parent)) = path.split_last() else {
                *target = value.clone();
                return Ok(());
            };
            object_at(target, parent)
                .ok_or_else(|| PatchError::MissingParent(path.clone()))?
                .insert(key.clone(), value.clone());
        }
        PatchOp::Remove { path } => {
            let (key, parent) = path.split_last().ok_or(PatchError::RemoveRoot)?;
            object_at(target, parent)
                .and_then(|o| o.remove(key))
                .ok_or_else(|| PatchError::MissingKey(path.clone()))?;
        }
    }
    Ok(())
}

fn object_at<'a>(mut node: &'a mut Value, path: &[String]) -> Option<&'a mut Map<String, Value>> {
    for key in path {
        node = node.as_object_mut()?.get_mut(key)?;
    }
    node.as_object_mut()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn path(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn diff_reaches_into_objects_and_replaces_the_rest() {
        let old = json!({ "config": { "huVolume": 0.5, "kiosk": { "main": false } }, "list": [1, 2], "gone": 1 });
        let new = json!({ "config": { "huVolume": 0.8, "kiosk": { "main": false } }, "list": [1, 2, 3], "added": null });
        assert_eq!(
            diff(&old, &new),
            vec![
                PatchOp::Set {
                    path: path(&["added"]),
                    value: Value::Null
                },
                PatchOp::Set {
                    path: path(&["config", "huVolume"]),
                    value: json!(0.8)
                },
                PatchOp::Set {
                    path: path(&["list"]),
                    value: json!([1, 2, 3])
                },
                PatchOp::Remove {
                    path: path(&["gone"])
                },
            ]
        );
    }

    #[test]
    fn equal_trees_give_no_ops() {
        let tree = json!({ "a": { "b": [1, { "c": true }] } });
        assert!(diff(&tree, &tree.clone()).is_empty());
    }

    #[test]
    fn applying_a_diff_reproduces_the_new_tree() {
        let old = json!({ "front": { "main": "livi" }, "config": { "a": 1, "b": { "c": 2 } } });
        let new =
            json!({ "front": { "main": "projection" }, "config": { "b": { "c": 3, "d": [4] } } });
        let mut copy = old.clone();
        for op in diff(&old, &new) {
            apply(&mut copy, &op).unwrap();
        }
        assert_eq!(copy, new);
    }

    #[test]
    fn a_changed_root_is_set_whole() {
        let mut target = json!(1);
        let ops = diff(&target, &json!({ "a": 1 }));
        assert_eq!(
            ops,
            vec![PatchOp::Set {
                path: vec![],
                value: json!({ "a": 1 })
            }]
        );
        apply(&mut target, &ops[0]).unwrap();
        assert_eq!(target, json!({ "a": 1 }));
    }

    #[test]
    fn a_patch_that_does_not_fit_is_an_error() {
        let mut target = json!({ "a": { "b": 1 } });
        assert_eq!(
            apply(
                &mut target,
                &PatchOp::Set {
                    path: path(&["x", "y"]),
                    value: json!(1)
                }
            ),
            Err(PatchError::MissingParent(path(&["x", "y"])))
        );
        assert_eq!(
            apply(
                &mut target,
                &PatchOp::Set {
                    path: path(&["a", "b", "c"]),
                    value: json!(1)
                }
            ),
            Err(PatchError::MissingParent(path(&["a", "b", "c"])))
        );
        assert_eq!(
            apply(
                &mut target,
                &PatchOp::Remove {
                    path: path(&["a", "z"])
                }
            ),
            Err(PatchError::MissingKey(path(&["a", "z"])))
        );
        assert_eq!(
            apply(&mut target, &PatchOp::Remove { path: vec![] }),
            Err(PatchError::RemoveRoot)
        );
    }

    #[test]
    fn ops_are_tagged_on_the_wire() {
        let set = PatchOp::Set {
            path: path(&["a"]),
            value: json!(1),
        };
        assert_eq!(
            serde_json::to_value(&set).unwrap(),
            json!({ "op": "set", "path": ["a"], "value": 1 })
        );
        let remove = PatchOp::Remove { path: path(&["a"]) };
        assert_eq!(
            serde_json::to_value(&remove).unwrap(),
            json!({ "op": "remove", "path": ["a"] })
        );
    }
}
