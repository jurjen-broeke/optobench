//! What alignment should achieve: the `[[merit]]` tables of a layout or sidecar, read by
//! the `optimize` command (`optobench_optimize`).
//!
//! ```toml
//! [[merit]]
//! kind = "centring"                 # or "focus", "pointing", "collimation", "diameter",
//!                                   # "power", "backscatter" (at a returning detector:
//!                                   # the light coming back, % of the launched power)
//! detector = "screen"
//! target = [0, 0]                   # centring: mm in the detector's frame; pointing: mrad
//! weight = 1.0                      # optional; residuals are mm, mrad or a fraction
//!
//! [[merit]]
//! kind = "diameter"                 # the direct beam's diameter (2√2 × its RMS radius:
//! detector = "screen"               # an even disc's edge, a Gaussian's 1/e²)
//! target = 5.0                      # mm, required
//!
//! [[merit]]
//! kind = "headroom"                 # adjusters near the middle of their ranges (no detector):
//! weight = 0.01                     # (value − middle) / half range per free DOF with a
//!                                   # range; default weight 0.01, so it decides only where
//!                                   # the optics leave a choice
//! ```

use optobench_optimize::{Merit, Term};
use toml::{Table, Value};

use crate::scene_file::{FileError, Item, check_keys, error, items};

/// One merit term from a table (a file's `[[merit]]` or a command's parameter).
pub(crate) fn term(item: &Item) -> Result<Merit, FileError> {
    let kind = item.text("kind")?;
    if kind == "headroom" {
        check_keys(item.table, &["kind", "weight"], &item.context)?;
        let weight = item.optional_number("weight", 0.01)?;
        if !(weight > 0.0 && weight.is_finite()) {
            return Err(error(format!(
                "{}: `weight` must be positive",
                item.context
            )));
        }
        return Ok(Merit {
            term: Term::Headroom,
            detector: String::new(),
            weight,
        });
    }
    let targeted = matches!(kind, "centring" | "pointing" | "diameter");
    let mut keys = vec!["kind", "detector", "weight"];
    if targeted {
        keys.push("target");
    }
    check_keys(item.table, &keys, &item.context)?;
    let target = if kind == "diameter" {
        (item.positive("target")?, 0.0)
    } else if item.table.contains_key("target") {
        item.pair("target")?
    } else {
        (0.0, 0.0)
    };
    let term = match kind {
        "focus" => Term::Focus,
        "centring" => Term::Centring { target },
        "pointing" => Term::Pointing {
            target: (target.0 * 1e-3, target.1 * 1e-3),
        },
        "collimation" => Term::Collimation,
        "diameter" => Term::Diameter { target: target.0 },
        "power" => Term::Power,
        "backscatter" => Term::Backscatter,
        other => {
            return Err(error(format!(
                "{}: merit kind `{other}` is not known; use \"focus\", \"centring\", \"pointing\", \"collimation\", \"diameter\", \"power\", \"backscatter\" or \"headroom\"",
                item.context
            )));
        }
    };
    let weight = item.optional_number("weight", 1.0)?;
    if !(weight > 0.0 && weight.is_finite()) {
        return Err(error(format!(
            "{}: `weight` must be positive",
            item.context
        )));
    }
    Ok(Merit {
        term,
        detector: item.text("detector")?.to_owned(),
        weight,
    })
}

/// The `[[merit]]` tables of a file's top-level table.
pub fn read(root: &Table) -> Result<Vec<Merit>, FileError> {
    items(root, "merit")?
        .iter()
        .enumerate()
        .map(|(i, item)| {
            term(&Item {
                table: item.table,
                context: format!("[[merit]] number {}", i + 1),
            })
        })
        .collect()
}

/// The `[[merit]]` tables of a file's text.
pub fn parse(text: &str) -> Result<Vec<Merit>, FileError> {
    let root: Table = text
        .parse()
        .map_err(|e: toml::de::Error| error(format!("not valid TOML: {e}")))?;
    read(&root)
}

/// Merit terms from JSON-like values already converted to TOML tables (the `optimize`
/// command's `merit` parameter).
pub fn from_tables(tables: &[Value]) -> Result<Vec<Merit>, FileError> {
    tables
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let table = v
                .as_table()
                .ok_or_else(|| error(format!("merit number {} must be an object", i + 1)))?;
            term(&Item {
                table,
                context: format!("merit number {}", i + 1),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merit_tables_are_read_strictly() {
        let merit = parse(
            "[[merit]]\nkind = \"centring\"\ndetector = \"d\"\ntarget = [1, 2]\n\
             [[merit]]\nkind = \"pointing\"\ndetector = \"d\"\ntarget = [0.5, 0]\nweight = 2\n\
             [[merit]]\nkind = \"focus\"\ndetector = \"d\"\n",
        )
        .expect("valid");
        assert_eq!(merit[0].term, Term::Centring { target: (1.0, 2.0) });
        assert_eq!(
            merit[1].term,
            Term::Pointing {
                target: (0.5e-3, 0.0)
            }
        );
        assert_eq!(merit[1].weight, 2.0);
        assert_eq!(merit[2].term, Term::Focus);
        for (text, message) in [
            (
                "[[merit]]\nkind = \"focus\"\ndetector = \"d\"\ntarget = [0, 0]\n",
                "unknown key `target`",
            ),
            (
                "[[merit]]\nkind = \"sharpness\"\ndetector = \"d\"\n",
                "not known",
            ),
            ("[[merit]]\nkind = \"power\"\n", "missing `detector`"),
            (
                "[[merit]]\nkind = \"power\"\ndetector = \"d\"\nweight = 0\n",
                "positive",
            ),
        ] {
            let e = parse(text).expect_err("rejected");
            assert!(e.0.contains(message), "{text}: {}", e.0);
        }
    }
}
