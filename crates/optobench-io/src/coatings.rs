//! Coatings as part, layout and sidecar files write them.
//!
//! ```toml
//! coating = "uncoated"                       # bare interface: Fresnel reflection (default)
//! coating = "ideal"                          # perfect anti-reflection
//! coating = { curve = "dmlp730b" }           # a measured R/T curve (`import-curve`)
//! coating = { layers = [                     # a thin-film stack, from the air side to the glass
//!   { material = "mgf2", quarter_wave_um = 0.55 },   # λ/(4n) at 0.55 µm
//!   { material = 2.1, thickness_nm = 120 },          # names or indices
//! ] }
//! coatings = [ { curve = "mvf52a" }, "ideal" ]   # surface by surface, instead of `coating`
//! ```

use std::sync::Arc;

use optobench_library::{CoatingSpec, Glass, LayerSpec, LayerThickness};
use optobench_trace::Coating;
use optobench_trace::coating::{CoatingCurve, Layer, ThinFilm};
use toml::Value;

use crate::scene_file::{FileError, MaterialLookup, check_keys, error};

/// Looks up an imported R/T curve by name.
pub type CurveLookup<'a> = dyn Fn(&str) -> Result<CoatingCurve, String> + 'a;

/// A lookup for files read without a library: every curve is missing.
pub fn no_curves(name: &str) -> Result<CoatingCurve, String> {
    Err(format!(
        "no curve `{name}`: no data directory is set to look for imported curves"
    ))
}

/// Reads one coating.
pub fn parse(value: &Value, context: &str) -> Result<CoatingSpec, FileError> {
    let unknown = || {
        error(format!(
            "{context}: a coating is \"uncoated\", \"ideal\", {{ curve = \"<name>\" }} or {{ layers = [...] }}"
        ))
    };
    match value {
        Value::String(s) if s == "uncoated" => Ok(CoatingSpec::Uncoated),
        Value::String(s) if s == "ideal" => Ok(CoatingSpec::Ideal),
        Value::Table(table) => {
            check_keys(table, &["curve", "layers"], context)?;
            match (table.get("curve"), table.get("layers")) {
                (Some(Value::String(name)), None) => Ok(CoatingSpec::Curve(name.clone())),
                (None, Some(Value::Array(layers))) if !layers.is_empty() => layers
                    .iter()
                    .map(|layer| layer_spec(layer, context))
                    .collect::<Result<Vec<_>, _>>()
                    .map(CoatingSpec::Layers),
                _ => Err(unknown()),
            }
        }
        _ => Err(unknown()),
    }
}

fn layer_spec(value: &Value, context: &str) -> Result<LayerSpec, FileError> {
    let table = value.as_table().ok_or_else(|| {
        error(format!(
            "{context}: each layer is {{ material = …, thickness_nm = … }} or quarter_wave_um"
        ))
    })?;
    let context = format!("{context} layer");
    check_keys(
        table,
        &["material", "thickness_nm", "quarter_wave_um"],
        &context,
    )?;
    let material = match table.get("material") {
        Some(Value::String(name)) => Glass::Named(name.clone()),
        Some(Value::Float(n)) => Glass::Index(*n),
        Some(Value::Integer(n)) => Glass::Index(*n as f64),
        _ => {
            return Err(error(format!(
                "{context}: `material` is a name or an index"
            )));
        }
    };
    let positive = |key: &str| -> Result<Option<f64>, FileError> {
        match table.get(key) {
            None => Ok(None),
            Some(v) => {
                let n = v
                    .as_float()
                    .or_else(|| v.as_integer().map(|i| i as f64))
                    .filter(|n| *n > 0.0 && n.is_finite())
                    .ok_or_else(|| error(format!("{context}: `{key}` must be positive")))?;
                Ok(Some(n))
            }
        }
    };
    let thickness = match (positive("thickness_nm")?, positive("quarter_wave_um")?) {
        (Some(nm), None) => LayerThickness::Physical(nm * 1e-3),
        (None, Some(um)) => LayerThickness::QuarterWave(um),
        _ => {
            return Err(error(format!(
                "{context}: give `thickness_nm` or `quarter_wave_um`"
            )));
        }
    };
    Ok(LayerSpec {
        material,
        thickness,
    })
}

/// Reads `coatings`, one per surface.
pub fn parse_list(
    value: &Value,
    surfaces: usize,
    context: &str,
) -> Result<Vec<CoatingSpec>, FileError> {
    let list = value
        .as_array()
        .ok_or_else(|| error(format!("{context}: `coatings` is a list, one per surface")))?;
    if list.len() != surfaces {
        return Err(error(format!(
            "{context}: {surfaces} surfaces need {surfaces} coatings, not {}",
            list.len()
        )));
    }
    list.iter().map(|c| parse(c, context)).collect()
}

/// The optical coating a spec names.
pub fn resolve(
    spec: &CoatingSpec,
    materials: &MaterialLookup,
    curves: &CurveLookup,
) -> Result<Coating, String> {
    Ok(match spec {
        CoatingSpec::Uncoated => Coating::Uncoated,
        CoatingSpec::Ideal => Coating::Ideal,
        CoatingSpec::Curve(name) => Coating::Curve(Arc::new(curves(name)?)),
        CoatingSpec::Layers(layers) => Coating::Stack(Arc::new(ThinFilm {
            layers: layers
                .iter()
                .map(|layer| {
                    let material = crate::library::glass(&layer.material, materials)?;
                    let thickness = match layer.thickness {
                        LayerThickness::Physical(um) => um,
                        LayerThickness::QuarterWave(wavelength) => {
                            let n = material
                                .material()
                                .refractive_index(wavelength)
                                .map_err(|e| format!("quarter wave at {wavelength} µm: {e}"))?;
                            wavelength / (4.0 * n)
                        }
                    };
                    Ok(Layer {
                        thickness,
                        material,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        })),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Value {
        let table: toml::Table = format!("coating = {text}").parse().expect("TOML");
        table["coating"].clone()
    }

    #[test]
    fn coatings_are_read_and_resolved() {
        assert_eq!(
            parse(&value("\"ideal\""), "c").expect("valid"),
            CoatingSpec::Ideal
        );
        assert_eq!(
            parse(&value("{ curve = \"f\" }"), "c").expect("valid"),
            CoatingSpec::Curve("f".into())
        );
        let stack = parse(
            &value("{ layers = [{ material = 1.38, quarter_wave_um = 0.55 }, { material = \"x\", thickness_nm = 100 }] }"),
            "c",
        )
        .expect("valid");
        let CoatingSpec::Layers(layers) = &stack else {
            panic!("layers")
        };
        assert_eq!(layers[1].thickness, LayerThickness::Physical(0.1));
        for bad in [
            "\"shiny\"",
            "{ curve = \"f\", layers = [] }",
            "{ layers = [] }",
            "{ layers = [{ material = 1.38 }] }",
            "{ layers = [{ material = 1.38, thickness_nm = 1, quarter_wave_um = 1 }] }",
            "{ curv = \"f\" }",
        ] {
            assert!(parse(&value(bad), "c").is_err(), "{bad}");
        }
        // A quarter wave of MgF₂ (n = 1.38) at 0.55 µm is 0.55 / (4 · 1.38) µm thick.
        let materials = |name: &str| Err(format!("no `{name}`"));
        let Coating::Stack(film) = resolve(
            &CoatingSpec::Layers(vec![LayerSpec {
                material: Glass::Index(1.38),
                thickness: LayerThickness::QuarterWave(0.55),
            }]),
            &materials,
            &no_curves,
        )
        .expect("resolves") else {
            panic!("a stack")
        };
        assert!((film.layers[0].thickness - 0.55 / (4.0 * 1.38)).abs() < 1e-15);
        assert!(resolve(&CoatingSpec::Curve("f".into()), &materials, &no_curves).is_err());
        assert!(
            parse_list(&value("[\"ideal\"]"), 2, "c").is_err(),
            "two surfaces need two"
        );
    }
}
