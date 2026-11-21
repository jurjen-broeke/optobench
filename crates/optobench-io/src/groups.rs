//! Groups (M16): items that move together, once their places relative to each other are
//! found (the lenses of a beam expander, a relay).
//!
//! ```toml
//! [[element]]
//! id = "L1"
//! group = "expander"                # any items: elements, sources, detectors, proxies
//!
//! [[group]]                         # optional: the group's own degrees of freedom
//! id = "expander"
//! dof = [ { name = "expander_x", kind = "slide", axis = "x", range = [-5, 5] },
//!         { name = "expander_tip", kind = "tilt", axis = "y", pivot = [100, 0, 75],
//!           range = [-2, 2] } ]
//! ```
//!
//! The interface moves a group's members together (Move on any of them). A group's DOFs
//! are in the scene's frame (axes, and a tilt's `pivot`, which every member turns about
//! and which a tilt must give): each becomes the DOF of its first member that can carry
//! one (an element, source or detector), and every other such member follows it one to
//! one (`follows`, `<dof>_<member>`). A proxy in a group moves with the interface's Move
//! but not with the group's DOFs. The expansion is done on the file's tables before
//! anything else reads them.
//!
//! In a sidecar a group also takes `occurrences`: untagged CAD parts that move with it
//! (M18: a lens's mount, sliding along the rods with it), each following the group's DOFs
//! one to one (`<dof>_<occurrence>`).
//!
//! ```toml
//! [[group]]
//! id = "L1 assembly"
//! occurrences = ["cxy1a:1"]
//! dof = [ { name = "L1_rail", kind = "slide", axis = [0, 0, 1], range = "free" } ]
//! ```

use toml::{Table, Value};

use crate::scene_file::{FileError, check_keys, error};

/// Tables whose entries can be in a group, and those of them that carry DOFs.
const MEMBERS: &[&str] = &["element", "source", "detector", "proxy"];
const WITH_DOFS: &[&str] = &["element", "source", "detector"];

/// A DOF of a CAD occurrence a group moves: the occurrence's path and its DOF entry.
pub(crate) type OccurrenceDof = (String, Table);

/// Removes every `group = …` from items and the `[[group]]` tables, giving the groups'
/// DOFs to their members; returns the DOFs of the CAD parts groups name
/// (`occurrences`, allowed only when `cad` is true: in a sidecar).
pub(crate) fn expand(root: &mut Table, cad: bool) -> Result<Vec<OccurrenceDof>, FileError> {
    let mut parts: Vec<OccurrenceDof> = Vec::new();
    // Members, in file order: (table, index, id, group).
    let mut members: Vec<(&'static str, usize, String, String)> = Vec::new();
    for table in MEMBERS {
        let Some(items) = root.get_mut(*table).and_then(Value::as_array_mut) else {
            continue;
        };
        for (index, item) in items.iter_mut().enumerate() {
            let Some(item) = item.as_table_mut() else {
                continue;
            };
            let Some(group) = item.remove("group") else {
                continue;
            };
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_owned();
            let group = group
                .as_str()
                .filter(|g| !g.trim().is_empty())
                .ok_or_else(|| error(format!("[[{table}]] `{id}`: `group` names a group")))?;
            members.push((table, index, id, group.to_owned()));
        }
    }
    let Some(list) = root.remove("group") else {
        return Ok(parts);
    };
    let list = list
        .as_array()
        .cloned()
        .ok_or_else(|| error("`group` must be a list of tables ([[group]])"))?;
    let mut seen: Vec<String> = Vec::new();
    for (n, value) in list.iter().enumerate() {
        let context = format!("[[group]] number {}", n + 1);
        let table = value
            .as_table()
            .ok_or_else(|| error(format!("{context}: must be a table")))?;
        check_keys(table, &["id", "dof", "notes", "occurrences"], &context)?;
        let id = table
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| error(format!("{context}: missing `id`")))?
            .to_owned();
        if seen.contains(&id) {
            return Err(error(format!("{context}: a second group `{id}`")));
        }
        seen.push(id.clone());
        let context = format!("[[group]] `{id}`");
        let occurrences: Vec<String> = match table.get("occurrences") {
            None => Vec::new(),
            Some(_) if !cad => {
                return Err(error(format!(
                    "{context}: `occurrences` lists CAD parts, which only a sidecar has"
                )));
            }
            Some(v) => v
                .as_array()
                .and_then(|a| a.iter().map(|o| o.as_str().map(str::to_owned)).collect())
                .ok_or_else(|| {
                    error(format!(
                        "{context}: `occurrences` is a list of CAD occurrence paths"
                    ))
                })?,
        };
        let movable: Vec<&(&str, usize, String, String)> = members
            .iter()
            .filter(|m| m.3 == id && WITH_DOFS.contains(&m.0))
            .collect();
        let dofs: Vec<Table> = match table.get("dof") {
            None => Vec::new(),
            Some(v) => v
                .as_array()
                .and_then(|a| a.iter().map(|e| e.as_table().cloned()).collect())
                .ok_or_else(|| error(format!("{context}: `dof` is a list of tables")))?,
        };
        if dofs.is_empty() {
            continue;
        }
        let Some((leader, rest)) = movable.split_first() else {
            return Err(error(format!(
                "{context}: no element, source or detector is in it to carry its DOFs"
            )));
        };
        for dof in dofs {
            let name = dof
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| error(format!("{context}: a DOF without a `name`")))?
                .to_owned();
            if dof.contains_key("frame") || dof.contains_key("follows") {
                return Err(error(format!(
                    "{context} DOF `{name}`: a group's DOFs are in the scene's frame and lead; no `frame` or `follows`"
                )));
            }
            if dof.get("kind").and_then(Value::as_str) == Some("tilt") && !dof.contains_key("pivot")
            {
                return Err(error(format!(
                    "{context} DOF `{name}`: a group turns about one point: give its `pivot` (scene frame, mm)"
                )));
            }
            let mut lead = dof.clone();
            lead.insert("frame".into(), Value::String("scene".into()));
            add_dof(root, leader.0, leader.1, lead);
            let follower = |member: &str| {
                let mut follow = Table::new();
                for key in ["kind", "axis", "pivot"] {
                    if let Some(v) = dof.get(key) {
                        follow.insert(key.into(), v.clone());
                    }
                }
                follow.insert("name".into(), Value::String(format!("{name}_{member}")));
                follow.insert("frame".into(), Value::String("scene".into()));
                follow.insert("follows".into(), Value::String(name.clone()));
                follow
            };
            for member in rest {
                add_dof(root, member.0, member.1, follower(&member.2));
            }
            for occurrence in &occurrences {
                parts.push((occurrence.clone(), follower(occurrence)));
            }
        }
    }
    Ok(parts)
}

/// Adds a DOF entry to an item's `dof` list.
fn add_dof(root: &mut Table, table: &str, index: usize, dof: Table) {
    let Some(item) = root
        .get_mut(table)
        .and_then(Value::as_array_mut)
        .and_then(|a| a.get_mut(index))
        .and_then(Value::as_table_mut)
    else {
        return;
    };
    match item.get_mut("dof").and_then(Value::as_array_mut) {
        Some(list) => list.push(Value::Table(dof)),
        None => {
            item.insert("dof".into(), Value::Array(vec![Value::Table(dof)]));
        }
    }
}
