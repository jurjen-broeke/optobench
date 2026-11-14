//! Editing a layout or sidecar file in place, keeping its comments and layout (the user
//! interface's editors, M8c; the `edit` command). Operations name what they change by
//! table and id (`[[element]]` `L2`), by table and index (`[[merit]]` 0, which has no
//! id), or by degree-of-freedom name, and carry values as JSON. The `[resolved]` section
//! stays the last section of the file. Checking that the result still reads as a scene
//! is the caller's (it needs the user's data).

use serde_json::Value as Json;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::resolved;

/// An edit failed: the operation, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditError(pub String);

impl core::fmt::Display for EditError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditError {}

fn error(message: impl Into<String>) -> EditError {
    EditError(message.into())
}

/// The tables whose entries are edited by id.
const WITH_ID: &[&str] = &["element", "source", "detector", "proxy", "group"];
/// The tables whose entries are edited by index.
const BY_INDEX: &[&str] = &["merit"];

/// A new layout file: the header and, for a table layout, the breadboard pitch.
pub fn new_layout(layout: &str, pitch: Option<f64>) -> Result<String, EditError> {
    let mut text = format!("format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"{layout}\"\n");
    match layout {
        "planar" => {}
        "table" => {
            text.push_str(&format!("\n[table]\npitch = {:?}\n", pitch.unwrap_or(25.0)));
        }
        other => {
            return Err(error(format!(
                "layout `{other}`: a new layout is `planar` or `table`"
            )));
        }
    }
    Ok(text)
}

/// A JSON value as a TOML value: numbers stay integers when they are whole in the JSON,
/// objects become inline tables.
fn value(json: &Json) -> Result<Value, EditError> {
    Ok(match json {
        Json::Bool(b) => Value::from(*b),
        Json::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => Value::from(i),
            (None, Some(f)) if f.is_finite() => Value::from(f),
            _ => return Err(error(format!("{n} is not a number TOML can hold"))),
        },
        Json::String(s) => Value::from(s.as_str()),
        Json::Array(items) => {
            let mut array = Array::new();
            for item in items {
                array.push(value(item)?);
            }
            Value::Array(array)
        }
        Json::Object(map) => {
            let mut table = InlineTable::new();
            for (k, v) in map {
                table.insert(k, value(v)?);
            }
            Value::InlineTable(table)
        }
        Json::Null => return Err(error("null is not a value: use `unset` to remove a key")),
    })
}

/// An entry's keys and values in the order to write them: from a list of `[key, value]`
/// pairs as given, or from an object with `id` and `kind` first and the rest in name
/// order.
fn pairs(entry: &Json) -> Result<Vec<(String, Json)>, EditError> {
    if let Some(list) = entry.as_array() {
        return list
            .iter()
            .map(|p| match p.as_array().map(Vec::as_slice) {
                Some([Json::String(k), v]) => Ok((k.clone(), v.clone())),
                _ => Err(error("`entry` pairs must be [key, value]")),
            })
            .collect();
    }
    let map = entry
        .as_object()
        .ok_or_else(|| error("`entry` must be an object or a list of [key, value] pairs"))?;
    let rank = |k: &str| match k {
        "id" => 0,
        "kind" => 1,
        _ => 2,
    };
    let mut out: Vec<(String, Json)> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    out.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then(a.0.cmp(&b.0)));
    Ok(out)
}

fn text_field<'a>(op: &'a Json, key: &str) -> Result<&'a str, EditError> {
    op[key]
        .as_str()
        .ok_or_else(|| error(format!("`{key}` must be text")))
}

fn entries<'a>(doc: &'a mut DocumentMut, table: &str) -> Result<&'a mut ArrayOfTables, EditError> {
    if !WITH_ID.contains(&table) && !BY_INDEX.contains(&table) {
        return Err(error(format!(
            "table `{table}`: one of {}",
            [WITH_ID, BY_INDEX].concat().join(", ")
        )));
    }
    doc.get_mut(table)
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| error(format!("the file has no [[{table}]]")))
}

/// The sections of one table each that operations may change by `section` (created when
/// missing): a table layout's breadboard, a planar layout's axis, the trace options.
const SECTIONS: &[&str] = &["table", "planar", "trace"];

/// The entry an operation names: `table` with `id`, or with `index`; a `section`
/// (`[table]`); or the file's top level when it names neither.
fn entry<'a>(doc: &'a mut DocumentMut, op: &Json) -> Result<&'a mut Table, EditError> {
    if let Some(section) = op.get("section") {
        let section = section
            .as_str()
            .filter(|s| SECTIONS.contains(s))
            .ok_or_else(|| error(format!("`section`: one of {}", SECTIONS.join(", "))))?;
        if doc.get(section).is_none() {
            doc.insert(section, Item::Table(Table::new()));
        }
        return doc
            .get_mut(section)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| error(format!("[{section}] is not a table")));
    }
    let Some(table) = op.get("table") else {
        return Ok(doc.as_table_mut());
    };
    let table = table
        .as_str()
        .ok_or_else(|| error("`table` must be text"))?
        .to_owned();
    let list = entries(doc, &table)?;
    if let Some(id) = op.get("id") {
        let id = id.as_str().ok_or_else(|| error("`id` must be text"))?;
        return list
            .iter_mut()
            .find(|t| t.get("id").and_then(Item::as_str) == Some(id))
            .ok_or_else(|| error(format!("no [[{table}]] with id `{id}`")));
    }
    let index = op["index"]
        .as_u64()
        .ok_or_else(|| error(format!("[[{table}]]: give `id` or `index`")))?
        as usize;
    let count = list.len();
    list.get_mut(index)
        .ok_or_else(|| error(format!("[[{table}]] has {count} entries, no index {index}")))
}

/// Where a degree of freedom is declared: table, entry and position in its `dof` list.
fn find_dof(doc: &DocumentMut, name: &str) -> Option<(&'static str, usize, usize)> {
    for table in WITH_ID {
        let Some(list) = doc.get(table).and_then(Item::as_array_of_tables) else {
            continue;
        };
        for (e, entry) in list.iter().enumerate() {
            let Some(dofs) = entry.get("dof").and_then(Item::as_array) else {
                continue;
            };
            let found = dofs.iter().position(|d| {
                d.as_inline_table()
                    .and_then(|t| t.get("name"))
                    .and_then(Value::as_str)
                    == Some(name)
            });
            if let Some(d) = found {
                return Some((table, e, d));
            }
        }
    }
    None
}

/// The inline table of a degree of freedom, by name, wherever it is declared.
fn dof<'a>(doc: &'a mut DocumentMut, name: &str) -> Result<&'a mut InlineTable, EditError> {
    let missing = || error(format!("no degree of freedom named `{name}`"));
    let (table, e, d) = find_dof(doc, name).ok_or_else(missing)?;
    doc.get_mut(table)
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|list| list.get_mut(e))
        .and_then(|entry| entry.get_mut("dof"))
        .and_then(Item::as_array_mut)
        .and_then(|dofs| dofs.get_mut(d))
        .and_then(Value::as_inline_table_mut)
        .ok_or_else(missing)
}

/// Ids in use across the tables with ids.
fn ids(doc: &DocumentMut) -> Vec<String> {
    WITH_ID
        .iter()
        .filter_map(|t| doc.get(t).and_then(Item::as_array_of_tables))
        .flat_map(|list| list.iter())
        .filter_map(|t| t.get("id").and_then(Item::as_str).map(str::to_owned))
        .collect()
}

fn apply(doc: &mut DocumentMut, op: &Json) -> Result<(), EditError> {
    let kind = text_field(op, "op")?;
    match kind {
        "set" => {
            let key = text_field(op, "key")?;
            let new = value(&op["value"])?;
            if let Some(name) = op.get("dof") {
                let name = name.as_str().ok_or_else(|| error("`dof` must be text"))?;
                dof(doc, name)?.insert(key, new);
                return Ok(());
            }
            if key == "id" && op.get("table").is_some() {
                let id = new.as_str().unwrap_or_default().to_owned();
                if ids(doc).contains(&id) {
                    return Err(error(format!("the id `{id}` is already used")));
                }
            }
            let table = entry(doc, op)?;
            // Keep the value's place and the comments around it.
            match table.get_mut(key).and_then(Item::as_value_mut) {
                Some(old) => {
                    let decor = old.decor().clone();
                    *old = new;
                    *old.decor_mut() = decor;
                }
                None => {
                    table.insert(key, Item::Value(new));
                }
            }
            Ok(())
        }
        "unset" => {
            let key = text_field(op, "key")?;
            if let Some(name) = op.get("dof") {
                let name = name.as_str().ok_or_else(|| error("`dof` must be text"))?;
                dof(doc, name)?.remove(key);
                return Ok(());
            }
            entry(doc, op)?
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| error(format!("no key `{key}` to unset")))
        }
        "add" => {
            let table_name = text_field(op, "table")?.to_owned();
            let fields = pairs(&op["entry"])?;
            if WITH_ID.contains(&table_name.as_str()) {
                let id = fields
                    .iter()
                    .find(|(k, _)| k == "id")
                    .and_then(|(_, v)| v.as_str())
                    .ok_or_else(|| error(format!("a new [[{table_name}]] needs an `id`")))?;
                if ids(doc).iter().any(|i| i == id) {
                    return Err(error(format!("the id `{id}` is already used")));
                }
            }
            let mut table = Table::new();
            for (k, v) in &fields {
                table.insert(k, Item::Value(value(v)?));
            }
            if doc.get(&table_name).is_none() {
                doc.insert(&table_name, Item::ArrayOfTables(ArrayOfTables::new()));
            }
            entries(doc, &table_name)?.push(table);
            Ok(())
        }
        "remove" => {
            let table_name = text_field(op, "table")?.to_owned();
            let index = {
                let list = entries(doc, &table_name)?;
                match (op.get("id"), op["index"].as_u64()) {
                    (Some(id), _) => {
                        let id = id.as_str().ok_or_else(|| error("`id` must be text"))?;
                        list.iter()
                            .position(|t| t.get("id").and_then(Item::as_str) == Some(id))
                            .ok_or_else(|| error(format!("no [[{table_name}]] with id `{id}`")))?
                    }
                    (None, Some(i)) if (i as usize) < list.len() => i as usize,
                    _ => return Err(error(format!("[[{table_name}]]: give `id` or `index`"))),
                }
            };
            let list = entries(doc, &table_name)?;
            list.remove(index);
            if list.is_empty() {
                doc.remove(&table_name);
            }
            Ok(())
        }
        "add_dof" => {
            // Pairs keep the order given; an object is written name, kind first.
            let fields = pairs(&op["dof_entry"]).map_err(|_| {
                error("`dof_entry` must be an object or a list of [key, value] pairs")
            })?;
            let name = fields
                .iter()
                .find(|(k, _)| k == "name")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| error("a new degree of freedom needs a `name`"))?
                .to_owned();
            if dof(doc, &name).is_ok() {
                return Err(error(format!(
                    "a degree of freedom `{name}` already exists"
                )));
            }
            let mut new = InlineTable::new();
            for (k, v) in &fields {
                new.insert(k, value(v)?);
            }
            let table = entry(doc, op)?;
            match table.get_mut("dof").and_then(Item::as_array_mut) {
                Some(list) => list.push(new),
                None => {
                    let mut list = Array::new();
                    list.push(new);
                    table.insert("dof", Item::Value(Value::Array(list)));
                }
            }
            Ok(())
        }
        "remove_dof" => {
            let name = text_field(op, "dof")?;
            let (table, e, d) = find_dof(doc, name)
                .ok_or_else(|| error(format!("no degree of freedom named `{name}`")))?;
            if let Some(entry) = doc
                .get_mut(table)
                .and_then(Item::as_array_of_tables_mut)
                .and_then(|list| list.get_mut(e))
            {
                if let Some(dofs) = entry.get_mut("dof").and_then(Item::as_array_mut) {
                    dofs.remove(d);
                    if dofs.is_empty() {
                        entry.remove("dof");
                    }
                }
            }
            Ok(())
        }
        other => Err(error(format!(
            "operation `{other}`: one of set, unset, add, remove, add_dof, remove_dof, replace"
        ))),
    }
}

/// Applies operations to a file's text, in order; returns the new text. The `[resolved]`
/// section is kept at the end, unchanged. A `replace` operation (`text`) replaces the
/// whole file, for undoing.
pub fn edit(text: &str, operations: &[Json]) -> Result<String, EditError> {
    if let [only] = operations {
        if only["op"].as_str() == Some("replace") {
            return only["text"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| error("`replace` needs the `text`"));
        }
    }
    let (body, embedded) = resolved::split(text).map_err(|e| error(e.0))?;
    let mut doc: DocumentMut = body
        .parse()
        .map_err(|e: toml_edit::TomlError| error(format!("the file is not valid TOML: {e}")))?;
    for (i, op) in operations.iter().enumerate() {
        apply(&mut doc, op).map_err(|e| error(format!("operation {}: {}", i + 1, e.0)))?;
    }
    let mut out = doc.to_string();
    if let Some(section) = embedded {
        if !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(section);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LAYOUT: &str = "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"table\"\n\n\
        # The breadboard.\n[table]\npitch = 25.0\n\n\
        [[element]]\nid = \"M1\"  # the first mirror\nkind = \"mirror\"\nhole = [4, 0]\nheight = 50.0\n\
        dof = [ { name = \"M1_y\", kind = \"tilt\", axis = [0, 1, 0], range = [-2, 2] } ]\n\n\
        [[merit]]\nkind = \"centring\"\ndetector = \"screen\"\n";

    #[test]
    fn sections_are_set_in_place_or_made() {
        let out = edit(
            LAYOUT,
            &[
                json!({"op": "set", "section": "table", "key": "pitch", "value": 25.4}),
                json!({"op": "set", "section": "table", "key": "columns", "value": 24}),
                json!({"op": "set", "section": "trace", "key": "threads", "value": 1}),
            ],
        )
        .expect("edits");
        assert!(
            out.contains("# The breadboard.\n[table]\npitch = 25.4\n"),
            "{out}"
        );
        assert!(out.contains("columns = 24"));
        assert!(out.contains("[trace]\nthreads = 1"), "{out}");
        let back = edit(
            &out,
            &[json!({"op": "unset", "section": "table", "key": "columns"})],
        )
        .expect("edits");
        assert!(!back.contains("columns"));
        let e = edit(
            LAYOUT,
            &[json!({"op": "set", "section": "element", "key": "x", "value": 1})],
        )
        .expect_err("not a section");
        assert!(e.0.contains("one of table, planar, trace"), "{}", e.0);
    }

    #[test]
    fn set_keeps_comments_and_the_rest_of_the_file() {
        let out = edit(
            LAYOUT,
            &[json!({"op": "set", "table": "element", "id": "M1", "key": "height", "value": 62.5})],
        )
        .expect("edits");
        assert!(out.contains("height = 62.5"));
        assert!(out.contains("# The breadboard."));
        assert!(out.contains("id = \"M1\"  # the first mirror"));
        assert_eq!(out.replace("height = 62.5", "height = 50.0"), LAYOUT);
    }

    #[test]
    fn whole_numbers_stay_whole_and_new_keys_are_added() {
        let out = edit(
            LAYOUT,
            &[
                json!({"op": "set", "table": "element", "id": "M1", "key": "hole", "value": [5, 1]}),
                json!({"op": "set", "table": "element", "id": "M1", "key": "diameter", "value": 25.4}),
            ],
        )
        .expect("edits");
        assert!(out.contains("hole = [5, 1]"));
        assert!(out.contains("diameter = 25.4"));
    }

    #[test]
    fn entries_are_added_and_removed_by_id_and_index() {
        let out = edit(
            LAYOUT,
            &[
                json!({"op": "add", "table": "detector", "entry": {"id": "screen", "hole": [8, 0], "height": 150.0, "axis": [1, 0, 0], "size": [20, 20]}}),
                json!({"op": "add", "table": "merit", "entry": {"kind": "pointing", "detector": "screen"}}),
                json!({"op": "remove", "table": "merit", "index": 0}),
            ],
        )
        .expect("edits");
        assert!(
            out.contains("[[detector]]\nid = \"screen\"\naxis = [1, 0, 0]"),
            "{out}"
        );
        let ordered = edit(
            LAYOUT,
            &[json!({"op": "add", "table": "source", "entry": [["id", "laser"], ["kind", "collimated"], ["hole", [0, 0]], ["height", 50.0]]})],
        )
        .expect("edits");
        assert!(
            ordered.contains("id = \"laser\"\nkind = \"collimated\"\nhole = [0, 0]\nheight = 50.0"),
            "{ordered}"
        );
        assert_eq!(out.matches("[[merit]]").count(), 1);
        assert!(out.contains("kind = \"pointing\"") && !out.contains("kind = \"centring\""));
        let gone = edit(
            &out,
            &[json!({"op": "remove", "table": "element", "id": "M1"})],
        )
        .expect("edits");
        assert!(!gone.contains("[[element]]"));
    }

    #[test]
    fn degrees_of_freedom_are_edited_by_name() {
        let out = edit(
            LAYOUT,
            &[
                json!({"op": "set", "dof": "M1_y", "key": "value", "value": 0.25}),
                json!({"op": "add_dof", "table": "element", "id": "M1", "dof_entry": {"name": "M1_z", "kind": "slide", "axis": [0, 0, 1], "range": [-5, 5]}}),
            ],
        )
        .expect("edits");
        assert!(out.contains("value = 0.25"), "{out}");
        assert!(out.contains("name = \"M1_z\""));
        let out = edit(&out, &[json!({"op": "remove_dof", "dof": "M1_y"})]).expect("edits");
        assert!(!out.contains("M1_y") && out.contains("M1_z"));
        let out = edit(&out, &[json!({"op": "remove_dof", "dof": "M1_z"})]).expect("edits");
        assert!(!out.contains("dof ="));
    }

    #[test]
    fn mistakes_are_refused_with_a_reason() {
        let cases = [
            (
                json!({"op": "set", "table": "element", "id": "M9", "key": "height", "value": 1}),
                "no [[element]] with id `M9`",
            ),
            (
                json!({"op": "add", "table": "element", "entry": {"id": "M1"}}),
                "already used",
            ),
            (
                json!({"op": "set", "table": "element", "id": "M1", "key": "id", "value": "M1"}),
                "already used",
            ),
            (
                json!({"op": "set", "dof": "nope", "key": "value", "value": 1}),
                "no degree of freedom",
            ),
            (
                json!({"op": "remove", "table": "merit", "index": 5}),
                "give `id` or `index`",
            ),
            (
                json!({"op": "set", "table": "thing", "id": "x", "key": "k", "value": 1}),
                "table `thing`",
            ),
            (json!({"op": "fly"}), "operation `fly`"),
            (
                json!({"op": "set", "key": "x", "value": null}),
                "use `unset`",
            ),
        ];
        for (op, expected) in cases {
            let e = edit(LAYOUT, std::slice::from_ref(&op)).expect_err("refused");
            assert!(e.0.contains(expected), "{op}: {e}");
        }
    }

    #[test]
    fn the_resolved_section_stays_last() {
        let text = format!(
            "{LAYOUT}\n{}\n[resolved.material.\"glass\"]\nkind = \"constant\"\nindex = 1.5\n",
            resolved::COMMENT.trim_end()
        );
        let out = edit(
            &text,
            &[json!({"op": "add", "table": "source", "entry": {"id": "laser", "kind": "collimated"}})],
        )
        .expect("edits");
        let source = out.find("[[source]]").expect("added");
        let section = out.find("[resolved.").expect("kept");
        assert!(source < section, "{out}");
        assert!(out.ends_with("index = 1.5\n"));
    }

    #[test]
    fn new_layouts_and_replacing() {
        let table = new_layout("table", Some(12.5)).expect("a layout");
        assert!(table.contains("layout = \"table\"") && table.contains("pitch = 12.5"));
        assert!(
            new_layout("planar", None)
                .expect("a layout")
                .ends_with("layout = \"planar\"\n")
        );
        assert!(new_layout("round", None).is_err());
        let back = edit(LAYOUT, &[json!({"op": "replace", "text": "x = 1\n"})]).expect("replaced");
        assert_eq!(back, "x = 1\n");
    }
}
