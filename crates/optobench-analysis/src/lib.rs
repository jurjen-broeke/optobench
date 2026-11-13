//! Paraxial analysis, spot diagrams, OPD and Zernike fits, PSF and MTF, power ledger, footprints, excitation and heating.
//!
//! Layer L4. See `docs/design.md`, section Architecture.
//!
//! M2 provides the image-quality analyses of a sequential system: pupils and ray aiming
//! ([`Analysis`]), spot diagrams, ray fans, optical path difference and Zernike fits.
//!
//! # Conventions
//!
//! - The object is at infinity; a field is a pair of angles (radians) of the incoming
//!   collimated beam, see [`Field`]. Finite conjugates come with scenes in M3.
//! - The last surface of the system is the image plane: a plane whose medium after it is
//!   the same as before it.
//! - Pupil coordinates (px, py) are normalised to the entrance pupil radius: the unit
//!   disk is the full pupil.
//! - Transverse results are in mm on the image plane, relative to the chief ray unless
//!   stated otherwise. OPD is in waves at the analysis wavelength.

#![forbid(unsafe_code)]

pub mod colour;
pub mod diffusion;
pub mod emission;
pub mod excitation;
mod focus;
mod gaussian;
pub mod graph;
pub mod heating;
mod psf;
mod pupil;
pub mod sample;
mod spot;
pub mod thermal;
mod wavefront;
mod zernike;

pub use focus::{Crossing, Focus, focus};
pub use gaussian::{BeamAt, gaussian};
pub use psf::{Polarization, Profile, Psf, PsfSettings, PupilWavefront, Section, psf};
pub use pupil::{Aiming, Analysis, AnalysisError, Field, ImageRay, Pupils};
pub use spot::{FanPoint, RayFan, SpotDiagram};
pub use wavefront::Wavefront;
pub use zernike::{ZernikeError, zernike, zernike_fit, zernike_indices};
