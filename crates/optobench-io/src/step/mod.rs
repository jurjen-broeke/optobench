//! STEP (ISO 10303) import: optobench's own reader for the AP203, AP214 and AP242
//! subset that mechanical CAD exports use for solids and assemblies.
//!
//! Two layers: [`part21`] reads the clear-text exchange structure, and [`model`] turns
//! it into products, the assembly tree with placements, and B-rep geometry in mm.
//! Decided 2026-11-07 (docs/design.md, Decided): no SolidCraft crates are used, so only
//! the STEP files SolidCraft writes matter, not its internals.

pub mod model;
pub mod part21;

pub use model::{
    BSplineCurve, BSplineSurface, Body, Child, Curve, Edge, Face, Header, Instance3d, Loop,
    ModelError, Product, StepModel, Surface,
};

/// Why a STEP file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    /// The text is not a valid exchange structure.
    Syntax(part21::ParseError),
    /// The entities do not form a model this reader understands.
    Model(ModelError),
}

impl core::fmt::Display for StepError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StepError::Syntax(e) => write!(f, "STEP syntax: {e}"),
            StepError::Model(e) => write!(f, "STEP model: {e}"),
        }
    }
}

impl std::error::Error for StepError {}

/// Reads a STEP file's text into a model.
pub fn read(text: &str) -> Result<StepModel, StepError> {
    let exchange = part21::parse(text).map_err(StepError::Syntax)?;
    model::model(&exchange).map_err(StepError::Model)
}
