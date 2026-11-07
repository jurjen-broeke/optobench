//! Reading the cells of a spreadsheet (`.xlsx`, Office Open XML, ECMA-376 Part 1), as
//! vendors publish measured curves: a ZIP archive (`zip`) whose workbook part lists the
//! sheets, each sheet part holds rows of cells, and text cells point into a shared
//! string table. Values only: formulas are read as their stored results, styles and
//! charts are ignored.

use crate::zip::Archive;

/// One piece of an XML document.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    /// `<name a="v">` (or `<name/>`, followed by `End`), the name without its prefix.
    Start(String, Vec<(String, String)>),
    /// `</name>`.
    End(String),
    /// Text between tags, entities resolved.
    Text(String),
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

/// Resolves the five predefined entities and character references (XML 1.0, 4.6).
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..end];
        let resolved = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match resolved {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The events of a document. Enough XML for spreadsheet parts: elements, attributes,
/// text, CDATA; declarations, comments and processing instructions are skipped.
fn events(xml: &str) -> Result<Vec<Event>, String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            out.push(Event::Text(unescape(rest)));
            break;
        };
        if open > 0 {
            out.push(Event::Text(unescape(&rest[..open])));
        }
        rest = &rest[open..];
        let skip = |rest: &str, end: &str| -> Result<usize, String> {
            rest.find(end)
                .map(|i| i + end.len())
                .ok_or_else(|| format!("unterminated `{}`", &rest[..rest.len().min(10)]))
        };
        if rest.starts_with("<!--") {
            rest = &rest[skip(rest, "-->")?..];
        } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").ok_or("unterminated CDATA")?;
            out.push(Event::Text(body[..end].to_owned()));
            rest = &body[end + 3..];
        } else if rest.starts_with("<?") {
            rest = &rest[skip(rest, "?>")?..];
        } else if rest.starts_with("<!") {
            rest = &rest[skip(rest, ">")?..];
        } else if let Some(body) = rest.strip_prefix("</") {
            let end = body.find('>').ok_or("unterminated end tag")?;
            out.push(Event::End(local(body[..end].trim())));
            rest = &body[end + 1..];
        } else {
            // A start tag: find its end outside quoted attribute values.
            let bytes = rest.as_bytes();
            let mut quote = None;
            let mut end = None;
            for (i, &b) in bytes.iter().enumerate().skip(1) {
                match (quote, b) {
                    (None, b'"' | b'\'') => quote = Some(b),
                    (Some(q), _) if b == q => quote = None,
                    (None, b'>') => {
                        end = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            let end = end.ok_or("unterminated start tag")?;
            let inner = &rest[1..end];
            let (inner, empty) = match inner.strip_suffix('/') {
                Some(i) => (i, true),
                None => (inner, false),
            };
            let name_end = inner
                .find(|c: char| c.is_whitespace())
                .unwrap_or(inner.len());
            let name = local(&inner[..name_end]);
            let mut attributes = Vec::new();
            let mut attrs = inner[name_end..].trim_start();
            while let Some(eq) = attrs.find('=') {
                let key = local(attrs[..eq].trim());
                let value = attrs[eq + 1..].trim_start();
                let q = value.chars().next().ok_or("attribute without a value")?;
                if q != '"' && q != '\'' {
                    return Err("unquoted attribute value".into());
                }
                let close = value[1..].find(q).ok_or("unterminated attribute value")?;
                attributes.push((key, unescape(&value[1..=close])));
                attrs = value[close + 2..].trim_start();
            }
            out.push(Event::Start(name.clone(), attributes));
            if empty {
                out.push(Event::End(name));
            }
            rest = &rest[end + 1..];
        }
    }
    Ok(out)
}

fn attribute<'a>(attributes: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// The 0-based column of a cell reference such as `C12`.
fn column_of(reference: &str) -> Option<usize> {
    let letters: String = reference
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    if letters.is_empty() {
        return None;
    }
    let n = letters.bytes().fold(0usize, |n, b| {
        n * 26 + usize::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    Some(n - 1)
}

fn text_part(archive: &Archive, name: &str) -> Result<String, String> {
    let bytes = archive.read(name).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| format!("`{name}` is not UTF-8"))
}

/// The sheets of a workbook: (name, part path).
fn sheets(archive: &Archive) -> Result<Vec<(String, String)>, String> {
    let workbook = events(&text_part(archive, "xl/workbook.xml")?)?;
    let relations = events(&text_part(archive, "xl/_rels/workbook.xml.rels")?)?;
    let target = |id: &str| {
        relations.iter().find_map(|e| match e {
            Event::Start(n, a) if n == "Relationship" && attribute(a, "Id") == Some(id) => {
                attribute(a, "Target").map(|t| match t.strip_prefix('/') {
                    Some(absolute) => absolute.to_owned(),
                    None => format!("xl/{t}"),
                })
            }
            _ => None,
        })
    };
    Ok(workbook
        .iter()
        .filter_map(|e| match e {
            Event::Start(n, a) if n == "sheet" => {
                let name = attribute(a, "name")?.to_owned();
                Some((name, target(attribute(a, "id")?)?))
            }
            _ => None,
        })
        .collect())
}

/// The cells of a sheet (the first, or the one named), as rows of text with empty
/// strings for empty cells, and the sheet's name.
pub fn read_sheet(data: &[u8], sheet: Option<&str>) -> Result<(String, Vec<Vec<String>>), String> {
    let archive = Archive::new(data).map_err(|e| e.to_string())?;
    let all = sheets(&archive)?;
    let (name, part) = match sheet {
        None => all.first().cloned().ok_or("the workbook has no sheets")?,
        Some(wanted) => all
            .iter()
            .find(|(n, _)| n == wanted)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "no sheet `{wanted}`; the sheets are {}",
                    all.iter()
                        .map(|(n, _)| format!("`{n}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?,
    };
    // Shared strings: each <si> is one string, the text of all its <t> pieces (rich
    // text runs), without phonetic guides (<rPh>).
    let mut shared = Vec::new();
    if archive.names().any(|n| n == "xl/sharedStrings.xml") {
        let mut current: Option<String> = None;
        let (mut in_t, mut in_phonetic) = (false, false);
        for event in events(&text_part(&archive, "xl/sharedStrings.xml")?)? {
            match event {
                Event::Start(n, _) if n == "si" => current = Some(String::new()),
                Event::Start(n, _) if n == "rPh" => in_phonetic = true,
                Event::End(n) if n == "rPh" => in_phonetic = false,
                Event::Start(n, _) if n == "t" => in_t = true,
                Event::End(n) if n == "t" => in_t = false,
                Event::Text(t) if in_t && !in_phonetic => {
                    if let Some(c) = current.as_mut() {
                        c.push_str(&t);
                    }
                }
                Event::End(n) if n == "si" => shared.push(current.take().unwrap_or_default()),
                _ => {}
            }
        }
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Option<Vec<String>> = None;
    let mut cell: Option<(usize, String, String)> = None; // column, type, value
    let mut in_value = false;
    for event in events(&text_part(&archive, &part)?)? {
        match event {
            Event::Start(n, _) if n == "row" => row = Some(Vec::new()),
            Event::End(n) if n == "row" => rows.push(row.take().unwrap_or_default()),
            Event::Start(n, a) if n == "c" => {
                let column = attribute(&a, "r")
                    .and_then(column_of)
                    .unwrap_or_else(|| row.as_ref().map_or(0, Vec::len));
                cell = Some((
                    column,
                    attribute(&a, "t").unwrap_or("n").to_owned(),
                    String::new(),
                ));
            }
            Event::Start(n, _) if n == "v" || n == "t" => in_value = true,
            Event::End(n) if n == "v" || n == "t" => in_value = false,
            Event::Text(t) if in_value => {
                if let Some(c) = cell.as_mut() {
                    c.2.push_str(&t);
                }
            }
            Event::End(n) if n == "c" => {
                if let (Some((column, kind, value)), Some(r)) = (cell.take(), row.as_mut()) {
                    let text = if kind == "s" {
                        value
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|i| shared.get(i).cloned())
                            .unwrap_or_default()
                    } else {
                        value
                    };
                    if r.len() <= column {
                        r.resize(column + 1, String::new());
                    }
                    r[column] = text;
                }
            }
            _ => {}
        }
    }
    Ok((name, rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_events() {
        let e = events("<?xml version=\"1.0\"?><!-- note --><a:x k='1 &amp; 2' m=\"&#65;\"><y/>t&lt;<![CDATA[<raw>]]></a:x>")
            .expect("parses");
        assert_eq!(
            e,
            vec![
                Event::Start(
                    "x".into(),
                    vec![("k".into(), "1 & 2".into()), ("m".into(), "A".into())]
                ),
                Event::Start("y".into(), vec![]),
                Event::End("y".into()),
                Event::Text("t<".into()),
                Event::Text("<raw>".into()),
                Event::End("x".into()),
            ]
        );
        assert!(events("<a b=c>").is_err());
    }

    #[test]
    fn cell_columns() {
        assert_eq!(column_of("A1"), Some(0));
        assert_eq!(column_of("E802"), Some(4));
        assert_eq!(column_of("AA3"), Some(26));
        assert_eq!(column_of("12"), None);
    }
}
