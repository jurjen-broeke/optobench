//! Mounts with adjusters (M14d): a mount's adjustment ranges written once, in a
//! `[[mount]]` table, and given to elements, sources and detectors by `mount = "<id>"`.
//!
//! ```toml
//! [[mount]]
//! id = "CXY1A"
//! source = "Thorlabs CXY1A specifications: ±1 mm in X and Y"
//! adjusters = [ { name = "x", kind = "slide", axis = "x", range = [-1, 1] },
//!               { name = "y", kind = "slide", axis = "y", range = [-1, 1] } ]
//!
//! [[mount]]
//! id = "MT1B"
//! source = "…: 12.7 mm of travel"
//! adjusters = [ { name = "z", kind = "slide", axis = "z", travel = 12.7 } ]
//!                                   # the whole travel, the part placed in its middle
//!
//! [[mount]]
//! id = "CRM1PT"
//! source = "…: 360° coarse, ±7° fine"
//! adjusters = [ { name = "coarse", kind = "tilt", axis = "z" },        # no range: unlimited
//!               { name = "fine", kind = "tilt", axis = "z", range = [-7, 7] } ]
//!
//! [[element]]
//! id = "L1"
//! mount = "CXY1A"                   # or a list, a mount on a mount: ["MT1B", "CXY1A"]
//! dof = [ { name = "L1_x", value = 0.2 } ]   # optional: changes to what the mount gives
//! ```
//!
//! An adjuster is a DOF entry (`dof`, see `scene_file`) without its item: it becomes the
//! DOF `<item id>_<name>` of every item that names the mount, in that item's frame as its
//! own DOFs are, with the mount's `source`. A DOF the item lists under the same name
//! changes the mount's entry key by key (a `value`, `locked`, another `range`); other
//! entries are added. The expansion is done on the file's tables before anything else
//! reads them.

use toml::{Table, Value};

use crate::scene_file::{FileError, check_keys, error};

/// Expands every `mount = …` in `[[element]]`, `[[source]]` and `[[detector]]` tables into
/// their `dof` lists and removes the `[[mount]]` tables.
pub(crate) fn expand(root: &mut Table) -> Result<(), FileError> {
    let Some(list) = root.remove("mount") else {
        return Ok(());
    };
    let list = list
        .as_array()
        .cloned()
        .ok_or_else(|| error("`mount` must be a list of tables ([[mount]])"))?;
    let mut mounts: Vec<(String, Table)> = Vec::new();
    for (n, value) in list.iter().enumerate() {
        let context = format!("[[mount]] number {}", n + 1);
        let table = value
            .as_table()
            .ok_or_else(|| error(format!("{context}: must be a table")))?;
        check_keys(table, &["id", "source", "adjusters"], &context)?;
        let id = table
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| error(format!("{context}: missing `id`")))?;
        if mounts.iter().any(|(m, _)| m == id) {
            return Err(error(format!("{context}: a second mount `{id}`")));
        }
        let context = format!("[[mount]] `{id}`");
        if !matches!(table.get("source"), Some(Value::String(s)) if !s.trim().is_empty()) {
            return Err(error(format!(
                "{context}: say where its ranges come from (`source`, its specification)"
            )));
        }
        let adjusters = table
            .get("adjusters")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                error(format!(
                    "{context}: `adjusters` is a list of {{ name, kind, axis, … }}"
                ))
            })?;
        for a in adjusters {
            let a = a
                .as_table()
                .ok_or_else(|| error(format!("{context}: each adjuster is a table")))?;
            check_keys(
                a,
                &["name", "kind", "axis", "frame", "pivot", "range", "travel"],
                &format!("{context} adjuster"),
            )?;
            if a.get("name").and_then(Value::as_str).is_none() {
                return Err(error(format!("{context}: an adjuster without a `name`")));
            }
        }
        mounts.push((id.to_owned(), table.clone()));
    }
    for kind in ["element", "source", "detector"] {
        let Some(items) = root.get_mut(kind).and_then(Value::as_array_mut) else {
            continue;
        };
        for item in items.iter_mut() {
            let Some(table) = item.as_table_mut() else {
                continue;
            };
            let Some(named) = table.remove("mount") else {
                continue;
            };
            let id = table
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_owned();
            let context = format!("[[{kind}]] `{id}`");
            let names: Vec<String> = match &named {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a
                    .iter()
                    .map(|v| v.as_str().map(str::to_owned))
                    .collect::<Option<_>>()
                    .ok_or_else(|| error(format!("{context}: `mount` names mounts")))?,
                _ => {
                    return Err(error(format!(
                        "{context}: `mount` names a mount or a list of them"
                    )));
                }
            };
            let own: Vec<Table> = match table.get("dof") {
                None => Vec::new(),
                Some(v) => v
                    .as_array()
                    .and_then(|a| a.iter().map(|e| e.as_table().cloned()).collect())
                    .ok_or_else(|| error(format!("{context}: `dof` is a list of tables")))?,
            };
            let mut entries: Vec<Table> = Vec::new();
            for name in &names {
                let (_, mount) = mounts
                    .iter()
                    .find(|(m, _)| m == name)
                    .ok_or_else(|| error(format!("{context}: no [[mount]] `{name}`")))?;
                for a in mount["adjusters"].as_array().into_iter().flatten() {
                    let mut entry = a.as_table().cloned().unwrap_or_default();
                    let adjuster = entry["name"].as_str().unwrap_or("?").to_owned();
                    entry.insert("name".into(), Value::String(format!("{id}_{adjuster}")));
                    entry.insert("source".into(), mount["source"].clone());
                    entries.push(entry);
                }
            }
            for mine in own {
                let name = mine.get("name").and_then(Value::as_str);
                match entries
                    .iter_mut()
                    .find(|e| e.get("name").and_then(Value::as_str) == name)
                {
                    Some(entry) => {
                        // Key by key; a `travel` replaces a `range` and the other way.
                        if mine.contains_key("range") {
                            entry.remove("travel");
                        }
                        if mine.contains_key("travel") {
                            entry.remove("range");
                        }
                        entry.extend(mine);
                    }
                    None => entries.push(mine),
                }
            }
            table.insert(
                "dof".into(),
                Value::Array(entries.into_iter().map(Value::Table).collect()),
            );
        }
    }
    Ok(())
}
