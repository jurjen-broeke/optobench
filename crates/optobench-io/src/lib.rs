//! The .optobench file format (sidecar and layout), STEP import, reports, and sources the user fetches.
//!
//! Layer L5. See `docs/design.md`, section Architecture.
//! M2: import of refractiveindex.info material files into the user data directory
//! ([`refractiveindex`], [`store`]). File formats and STEP follow in M3 and M4.

#![forbid(unsafe_code)]

pub mod placements;
pub mod refractiveindex;
pub mod scene_file;
mod sha256;
pub mod sidecar;
pub mod step;
pub mod store;
pub mod svg;

pub use sha256::{sha256, sha256_hex};

/// The file extension for every file optobench writes for people to keep.
pub const EXTENSION: &str = "optobench";

/// The format tag prefix: the first line of every `.optobench` file is
/// `format = "optobench/<N>"`.
pub const FORMAT_PREFIX: &str = "optobench/";

/// The current format version written by this release.
pub const FORMAT_VERSION: u32 = 1;

/// The kinds a `.optobench` file can hold, recorded in its `kind` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Optical roles, sources, detectors and degrees of freedom for a CAD design.
    Sidecar,
    /// A design without CAD: a planar or table layout.
    Layout,
}

impl Kind {
    /// The word written in the `kind` field.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Sidecar => "sidecar",
            Kind::Layout => "layout",
        }
    }
}

/// The format tag for the current version, e.g. `optobench/1`.
pub fn format_tag() -> String {
    format!("{FORMAT_PREFIX}{FORMAT_VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tag_is_versioned() {
        assert_eq!(format_tag(), "optobench/1");
        assert_eq!(Kind::Layout.as_str(), "layout");
    }
}
