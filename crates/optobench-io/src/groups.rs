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

use toml::{Table, Value};

use crate::scene_file::{FileError, check_keys, error};

/// Tables whose entries can be in a group, and those of them that carry DOFs.
const MEMBERS: &[&str] = &["element", "source", "detector", "proxy"];
const WITH_DOFS: &[&str] = &["element", "source", "detector"];

/// Removes every `group = …` from items and the `[[group]]` tables, giving the groups'
/// DOFs to their members.
pub(crate) fn expand(root: &mut Table) -> Result<(), FileError> {
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
        return Ok(());
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
        check_keys(table, &["id", "dof", "notes"], &context)?;
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
            for member in rest {
                let mut follow = Table::new();
                for key in ["kind", "axis", "pivot"] {
                    if let Some(v) = dof.get(key) {
                        follow.insert(key.into(), v.clone());
                    }
                }
                follow.insert("name".into(), Value::String(format!("{name}_{}", member.2)));
                follow.insert("frame".into(), Value::String("scene".into()));
                follow.insert("follows".into(), Value::String(name.clone()));
                add_dof(root, member.0, member.1, follow);
            }
        }
    }
    Ok(())
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
