//! The parts library in the user's data directory: part files and their import
//! (docs/design.md, Sidecar and library formats, "Parts library").
//!
//! ```text
//! <data directory>/optobench/library/parts/<vendor>/<part number>.toml
//! ```
//!
//! `<vendor>` is the vendor in lower case. The user builds the library from spec sheets
//! or imports part files, and moves it between installations; nothing is bundled.
//!
//! A part file:
//!
//! ```toml
//! format = "optobench/1"
//! kind = "part"
//! part = "LA1951-ML"
//! vendor = "Thorlabs"
//! type = "lens"                     # or "filter" (flat plate), "mirror"
//! source = "https://www.thorlabs.com/..."   # the spec sheet the numbers came from
//! diameter = 25.4                   # or size = [width, height] for a rectangular part
//! clear_aperture = 22.9             # optional
//! surfaces = [ { radius = 13.1 }, { radius = "inf" } ]   # optional conic; a filter: optional
//! thickness = [11.7]                # centre thicknesses (a mirror: its substrate, optional)
//! glass = ["N-BK7"]                 # names (bundled or imported) or indices
//! coating = "uncoated"              # optional: "uncoated" or "ideal"
//! products = ["ttn189114-e0w"]      # optional: other names of its STEP product
//! notes = "..."                     # optional
//! ```
//!
//! A STEP product named after the part number or one of `products` is the part: a
//! sidecar tags its glass bodies automatically (see `sidecar`).

use std::fs;
use std::path::{Path, PathBuf};

use optobench_library::{Glass, Library, Outline, Part, PartSurface, PartType};
use optobench_materials::Medium;
use toml::{Table, Value};

use crate::scene_file::{FileError, Item, MaterialLookup, check_header, check_keys, error};

/// The `kind` of a part file.
pub const KIND: &str = "part";

/// `<root>/optobench/library/parts`.
pub fn parts_directory(root: &Path) -> PathBuf {
    root.join("optobench").join("library").join("parts")
}

/// Reads a part file.
pub fn parse(text: &str) -> Result<Part, FileError> {
    check_header(text)?;
    let root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| error(format!("not valid TOML: {e}")))?;
    let item = Item {
        table: &root,
        context: format!(
            "part `{}`",
            root.get("part").and_then(Value::as_str).unwrap_or("?")
        ),
    };
    let format = item.text("format")?;
    if format != crate::format_tag() {
        return Err(error(format!(
            "format `{format}` is not the one this release reads ({})",
            crate::format_tag()
        )));
    }
    if item.text("kind")? != KIND {
        return Err(error("a part file has kind = \"part\""));
    }
    check_keys(
        &root,
        &[
            "format",
            "kind",
            "part",
            "vendor",
            "type",
            "source",
            "diameter",
            "size",
            "clear_aperture",
            "surfaces",
            "thickness",
            "glass",
            "coating",
            "products",
            "notes",
        ],
        &item.context,
    )?;
    let part_type = match item.text("type")? {
        "lens" => PartType::Lens,
        "filter" => PartType::Filter,
        "mirror" => PartType::Mirror,
        other => {
            return Err(error(format!(
                "{}: type `{other}` is not known; use \"lens\", \"filter\" or \"mirror\"",
                item.context
            )));
        }
    };
    let outline = match (root.get("diameter"), root.get("size")) {
        (Some(_), None) => Outline::Round {
            diameter: item.positive("diameter")?,
        },
        (None, Some(_)) => {
            let (width, height) = item.pair("size")?;
            Outline::Rectangular { width, height }
        }
        _ => {
            return Err(error(format!(
                "{}: give `diameter` (round) or `size` = [width, height]",
                item.context
            )));
        }
    };
    let surfaces = match root.get("surfaces") {
        None if part_type == PartType::Filter => vec![PartSurface::FLAT; 2],
        None => return Err(error(format!("{}: missing `surfaces`", item.context))),
        Some(value) => value
            .as_array()
            .ok_or_else(|| error(format!("{}: `surfaces` must be a list", item.context)))?
            .iter()
            .map(|s| {
                let table = s.as_table().ok_or_else(|| {
                    error(format!(
                        "{}: each surface is {{ radius = …, conic = … }}",
                        item.context
                    ))
                })?;
                let context = format!("{} surface", item.context);
                check_keys(table, &["radius", "conic"], &context)?;
                let surface = Item { table, context };
                Ok(PartSurface {
                    radius: surface.radius(surface.get("radius")?)?,
                    conic: surface.optional_number("conic", 0.0)?,
                })
            })
            .collect::<Result<Vec<_>, FileError>>()?,
    };
    let thickness = match root.get("thickness") {
        None if part_type == PartType::Mirror => Vec::new(),
        _ => item.numbers("thickness", None)?,
    };
    let glass = match root.get("glass") {
        None if part_type == PartType::Mirror => Vec::new(),
        None => return Err(error(format!("{}: missing `glass`", item.context))),
        Some(value) => value
            .as_array()
            .ok_or_else(|| error(format!("{}: `glass` must be a list", item.context)))?
            .iter()
            .map(|g| match g {
                Value::String(name) => Ok(Glass::Named(name.clone())),
                Value::Float(n) => Ok(Glass::Index(*n)),
                Value::Integer(n) => Ok(Glass::Index(*n as f64)),
                _ => Err(error(format!(
                    "{}: a glass is a name or an index",
                    item.context
                ))),
            })
            .collect::<Result<Vec<_>, FileError>>()?,
    };
    let coating = match root.get("coating") {
        None => None,
        Some(Value::String(name)) if name == "uncoated" || name == "ideal" => Some(name.clone()),
        Some(_) => {
            return Err(error(format!(
                "{}: coating is \"uncoated\" or \"ideal\"",
                item.context
            )));
        }
    };
    let products = match root.get("products") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .and_then(|a| {
                a.iter()
                    .map(|p| p.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| error(format!("{}: `products` is a list of names", item.context)))?,
    };
    if let Some(notes) = root.get("notes") {
        notes
            .as_str()
            .ok_or_else(|| error(format!("{}: `notes` must be text", item.context)))?;
    }
    let part = Part {
        vendor: item.text("vendor")?.to_owned(),
        number: item.text("part")?.to_owned(),
        part_type,
        source: item.text("source")?.to_owned(),
        outline,
        clear_aperture: match root.get("clear_aperture") {
            None => None,
            Some(_) => Some(item.positive("clear_aperture")?),
        },
        surfaces,
        thickness,
        glass,
        coating,
        products,
    };
    for (what, name) in [("vendor", part.vendor.as_str()), ("part", &part.number)] {
        if !valid_name(name) {
            return Err(error(format!(
                "{}: `{what}` may use letters, digits, `-`, `_`, `.` and `+`, starting with a letter or digit",
                item.context
            )));
        }
    }
    part.check()
        .map_err(|e| error(format!("{}: {e}", item.context)))?;
    Ok(part)
}

/// A vendor or part number that is safe as a file name.
fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
        && name.as_bytes()[0].is_ascii_alphanumeric()
}

/// Where a part's file belongs in the library.
pub fn part_path(root: &Path, part: &Part) -> PathBuf {
    parts_directory(root)
        .join(part.vendor.to_lowercase())
        .join(format!("{}.toml", part.number))
}

/// Every part in the library under `root`, each with its file. A missing library is an
/// empty one; a file that does not read, or sits in the wrong place, is an error naming it.
pub fn list(root: &Path) -> Result<Vec<(Part, PathBuf)>, String> {
    let directory = parts_directory(root);
    let mut found = Vec::new();
    let Ok(vendors) = fs::read_dir(&directory) else {
        return Ok(found);
    };
    let mut vendors: Vec<PathBuf> = vendors
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    vendors.sort();
    for vendor in vendors {
        let mut files: Vec<PathBuf> = fs::read_dir(&vendor)
            .map_err(|e| format!("{}: {e}", vendor.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .collect();
        files.sort();
        for file in files {
            let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let part = parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
            let expected = part_path(root, &part);
            if expected != file {
                return Err(format!(
                    "{}: part `{}` belongs at {}",
                    file.display(),
                    part.key(),
                    expected.display()
                ));
            }
            found.push((part, file));
        }
    }
    Ok(found)
}

/// The library under `root` (empty when there is none).
pub fn load(root: &Path) -> Result<Library, String> {
    let parts = list(root)?.into_iter().map(|(part, _)| part).collect();
    Library::new(parts).map_err(|e| format!("{}: {e}", parts_directory(root).display()))
}

/// Imports a part file into the library under `root`, unchanged. Importing the same file
/// again is harmless; a different file for a part already there needs `replace`.
pub fn import(root: &Path, path: &Path, replace: bool) -> Result<(Part, PathBuf), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let part = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let library = load(root)?;
    let destination = part_path(root, &part);
    if let Some(existing) = library.get(&part.key()) {
        let existing_path = part_path(root, existing);
        let same = fs::read_to_string(&existing_path).is_ok_and(|t| t == text);
        if !same && !replace {
            return Err(format!(
                "part `{}` is already in the library ({}); pass `replace` to overwrite it",
                part.key(),
                existing_path.display()
            ));
        }
        if existing_path != destination {
            fs::remove_file(&existing_path)
                .map_err(|e| format!("{}: {e}", existing_path.display()))?;
        }
    }
    // A clash of STEP product names with another part is refused before writing.
    let others: Vec<Part> = library
        .parts()
        .iter()
        .filter(|p| !p.has_key(&part.key()))
        .cloned()
        .chain([part.clone()])
        .collect();
    Library::new(others)?;
    let parent = destination.parent().unwrap_or(root);
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    fs::write(&destination, &text).map_err(|e| format!("{}: {e}", destination.display()))?;
    Ok((part, destination))
}

/// The medium of a part's glass. A name is looked up as written, then in lower case
/// (imported materials have lower-case names; spec sheets write `N-BK7`).
pub fn glass(glass: &Glass, lookup: &MaterialLookup) -> Result<Medium, String> {
    match glass {
        Glass::Index(n) => Ok((*n).into()),
        Glass::Named(name) => lookup(name).or_else(|first| {
            let lower = name.to_lowercase();
            if lower == *name {
                Err(first)
            } else {
                lookup(&lower).map_err(|_| first)
            }
        }),
    }
}

/// The coating a part names.
pub fn coating(part: &Part) -> optobench_trace::Coating {
    match part.coating.as_deref() {
        Some("ideal") => optobench_trace::Coating::Ideal,
        _ => optobench_trace::Coating::Uncoated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LENS: &str = "format = \"optobench/1\"\nkind = \"part\"\npart = \"PC-30\"\nvendor = \"Example\"\ntype = \"lens\"\nsource = \"hand-written test part\"\ndiameter = 25.4\nclear_aperture = 22.9\nsurfaces = [{ radius = 30 }, { radius = \"inf\" }]\nthickness = [5]\nglass = [\"N-BK7\"]\nproducts = [\"pc-lens\"]\n";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("optobench-library-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("scratch");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn part_files_are_read_strictly() {
        let part = parse(LENS).expect("valid");
        assert_eq!(part.key(), "example/PC-30");
        assert_eq!(part.glass, vec![Glass::Named("N-BK7".into())]);
        assert!(part.surfaces[1].radius.is_infinite());
        let filter = parse(
            "format = \"optobench/1\"\nkind = \"part\"\npart = \"F1\"\nvendor = \"Example\"\ntype = \"filter\"\nsource = \"s\"\nsize = [25, 36]\nthickness = [1.1]\nglass = [1.52]\n",
        )
        .expect("a filter needs no surfaces");
        assert_eq!(filter.surfaces, vec![PartSurface::FLAT; 2]);
        for (change, message) in [
            ("glass = [\"N-BK7\"]", "missing `glass`"),
            ("diameter = 25.4", "give `diameter`"),
            ("type = \"lens\"", "missing `type`"),
            ("thickness = [5]", "missing `thickness`"),
        ] {
            let text = LENS.replace(&format!("{change}\n"), "");
            let e = parse(&text).expect_err("rejected");
            assert!(e.0.contains(message), "{change}: {}", e.0);
        }
        let e = parse(&LENS.replace("clear_aperture", "clear_apperture")).expect_err("typo");
        assert!(e.0.contains("did you mean `clear_aperture`"), "{}", e.0);
        let e = parse(&LENS.replace("PC-30", "../PC-30")).expect_err("not a file name");
        assert!(e.0.contains("letters, digits"), "{}", e.0);
        let e = parse(&LENS.replace("thickness = [5]", "thickness = [5, 2]")).expect_err("count");
        assert!(e.0.contains("thicknesses"), "{}", e.0);
    }

    #[test]
    fn import_list_and_replace() {
        let scratch = Scratch::new("import");
        let root = &scratch.0;
        assert!(load(root).expect("no library yet").parts().is_empty());
        let file = root.join("pc-30.toml");
        fs::write(&file, LENS).expect("written");
        let (part, stored) = import(root, &file, false).expect("imported");
        assert_eq!(
            stored,
            parts_directory(root).join("example").join("PC-30.toml")
        );
        assert_eq!(fs::read_to_string(&stored).expect("stored"), LENS);
        // The same file again is harmless; a changed one needs `replace`.
        import(root, &file, false).expect("same file");
        fs::write(&file, LENS.replace("[5]", "[5.1]")).expect("written");
        let e = import(root, &file, false).expect_err("needs replace");
        assert!(e.contains("replace"), "{e}");
        import(root, &file, true).expect("replaced");
        let library = load(root).expect("loads");
        assert_eq!(library.parts().len(), 1);
        assert_eq!(
            library.get(&part.key()).expect("there").thickness,
            vec![5.1]
        );
        // A second part claiming the same STEP product is refused.
        fs::write(&file, LENS.replace("PC-30", "PC-31")).expect("written");
        let e = import(root, &file, false).expect_err("product clash");
        assert!(e.contains("pc-lens"), "{e}");
        // A file in the wrong place is reported.
        fs::write(
            parts_directory(root).join("example").join("wrong.toml"),
            LENS,
        )
        .expect("written");
        let e = load(root).expect_err("misplaced");
        assert!(e.contains("belongs at"), "{e}");
    }

    #[test]
    fn glass_names_fall_back_to_lower_case() {
        let lookup = |name: &str| -> Result<Medium, String> {
            if name == "n-bk7" {
                Ok(1.5168.into())
            } else {
                Err(format!("no material `{name}`"))
            }
        };
        assert!(glass(&Glass::Named("N-BK7".into()), &lookup).is_ok());
        let e = glass(&Glass::Named("N-SF6".into()), &lookup).expect_err("unknown");
        assert!(e.contains("`N-SF6`"), "{e}");
    }
}
