//! Adapter for the SolidCraft control channel (Phase B, milestone M12: read only). Every
//! SolidCraft-specific detail lives in this crate.
//!
//! Layer L5. See `docs/design.md`, section Architecture, Phase B, and
//! `docs/milestones/M12-live-read-only.md`.
//!
//! The channel, as SolidCraft documents it (`docs/control-protocol.md` in its repository,
//! MIT OR Apache-2.0; read 2026-11-20 for SolidCraft 0.45.0): plain TCP on 127.0.0.1, one
//! JSON object per line. A request is `{"id", "method", "params"}`; the first request on a
//! connection carries the launch's `token` too. A reply is `{"id", "ok": true, "result"}`
//! or `{"id", "ok": false, "error"}`, possibly with `busy`, `running` or `cancelled`. Lines
//! without an `id` are events, such as `{"event": "design.changed", "params":
//! {"document_id", "revision"}}` after `control.subscribe`.
//!
//! - [`discovery`]: where the running app listens, and its token.
//! - [`connection`]: one connection, its requests and its events.
//! - [`contract`]: the commands M12 reads, checked against the app's own schemas before
//!   anything is read; a changed contract refuses the link.
//! - [`read`]: occurrences with their transforms, joints, parameters; the design's
//!   revision; a STEP export of the design for tracing.
//! - [`live`]: a sidecar traced against the design, again after each change (the
//!   tracing done through `optobench-commands`, which knows nothing of SolidCraft).
//! - [`write`]: DOF values made in the design (M13): one batch, read back, taken back
//!   whole when anything differs.
//! - [`fake`]: a stand-in for the app in tests (no SolidCraft in CI).
//!
//! M12 changes nothing in the design. The STEP export writes a file into a folder the
//! caller gives, and every request names that folder as the only one allowed.

#![forbid(unsafe_code)]

pub mod connection;
pub mod contract;
pub mod discovery;
#[doc(hidden)]
pub mod fake;
pub mod live;
pub mod read;
pub mod write;

use std::fmt;

/// What can go wrong on the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// No running SolidCraft was found (or it does not let agents connect).
    NotFound(String),
    /// The connection failed or was closed.
    Connection(String),
    /// The token was refused.
    Unauthorized,
    /// The app is busy with another request on this connection.
    Busy(String),
    /// The request did not finish in time; it still runs in the app and will apply.
    StillRunning(String),
    /// The app answered with an error.
    Refused(String),
    /// A reply that does not read as the protocol says.
    Protocol(String),
    /// The app's commands are not the ones this version of optobench was written against.
    Contract(Vec<String>),
    /// What was written is not what was asked for (M13); the design was put back as it
    /// was.
    Mismatch(Vec<String>),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::NotFound(why) => write!(f, "no SolidCraft to connect to: {why}"),
            BridgeError::Connection(why) => write!(f, "the connection to SolidCraft failed: {why}"),
            BridgeError::Unauthorized => write!(
                f,
                "SolidCraft refused the token: restart SolidCraft, or remove stale token files"
            ),
            BridgeError::Busy(why) => write!(f, "SolidCraft is busy: {why}"),
            BridgeError::StillRunning(why) => {
                write!(
                    f,
                    "SolidCraft is still working on it (it will finish): {why}"
                )
            }
            BridgeError::Refused(why) => write!(f, "SolidCraft: {why}"),
            BridgeError::Protocol(why) => write!(f, "an unexpected reply from SolidCraft: {why}"),
            BridgeError::Mismatch(differences) => write!(
                f,
                "SolidCraft did not make the change asked for ({}); the design is as it was",
                differences.join("; ")
            ),
            BridgeError::Contract(changes) => write!(
                f,
                "this SolidCraft's commands differ from those optobench reads ({}); nothing was read",
                changes.join("; ")
            ),
        }
    }
}

impl std::error::Error for BridgeError {}
