//! Where a running SolidCraft listens, and its token.
//!
//! SolidCraft writes, while "Let AI agents connect" is on (Preferences › Assistant), a
//! token file `<port>.token` (one line of hex) per launch into its control folder, and
//! `current.port` (the port only) beside it. The folder is `SOLIDCRAFT_CONTROL_DIR` when
//! set, else on macOS `~/Library/Application Support/SolidCraft/control`, on Linux
//! `$XDG_DATA_HOME/solidcraft/control` (default `~/.local/share/solidcraft/control`), on
//! Windows `%LOCALAPPDATA%\SolidCraft\control`. `SOLIDCRAFT_CONTROL_PORT` with
//! `SOLIDCRAFT_CONTROL_TOKEN` name an app directly.
//!
//! A force-quit app leaves its files behind, so a token file is only a candidate: the one
//! `current.port` names first, then the others newest first, each tried until one
//! accepts a connection.

use std::fmt;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::BridgeError;

/// A running app's port and token. Its `Debug` and `Display` never show the token.
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The port on 127.0.0.1.
    pub port: u16,
    token: String,
}

impl Endpoint {
    /// An endpoint from a port and a token (taken as given, trimmed).
    pub fn new(port: u16, token: &str) -> Self {
        Self {
            port,
            token: token.trim().to_owned(),
        }
    }

    /// The token, for the first request on a connection.
    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// The address to connect to.
    pub fn address(&self) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], self.port))
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Endpoint {{ port: {}, token: <hidden> }}", self.port)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "127.0.0.1:{}", self.port)
    }
}

/// The control folder for this platform (see the module's documentation).
pub fn control_directory() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("SOLIDCRAFT_CONTROL_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        Some(home()?.join("Library/Application Support/SolidCraft/control"))
    } else if cfg!(windows) {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("SolidCraft\\control"))
    } else {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(home()?.join(".local/share")))?;
        Some(data.join("solidcraft/control"))
    }
}

/// The candidates in a control folder: the port `current.port` names first, then every
/// other `<port>.token`, newest first.
pub fn candidates(directory: &Path) -> Vec<Endpoint> {
    let current: Option<u16> = std::fs::read_to_string(directory.join("current.port"))
        .ok()
        .and_then(|t| t.trim().parse().ok());
    let mut found: Vec<(std::time::SystemTime, Endpoint)> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()? != "token" {
                return None;
            }
            let port: u16 = path.file_stem()?.to_str()?.parse().ok()?;
            let token = std::fs::read_to_string(&path).ok()?;
            if token.trim().is_empty() {
                return None;
            }
            let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
            Some((modified, Endpoint::new(port, &token)))
        })
        .collect();
    found.sort_by(|a, b| {
        let first = |e: &Endpoint| Some(e.port) != current;
        first(&a.1).cmp(&first(&b.1)).then(b.0.cmp(&a.0))
    });
    found.into_iter().map(|(_, e)| e).collect()
}

/// The running app: the environment's port and token when both are set, else the first
/// candidate of the control folder that accepts a connection within `timeout`.
pub fn find(timeout: Duration) -> Result<Endpoint, BridgeError> {
    if let (Ok(port), Ok(token)) = (
        std::env::var("SOLIDCRAFT_CONTROL_PORT"),
        std::env::var("SOLIDCRAFT_CONTROL_TOKEN"),
    ) {
        let port = port.trim().parse().map_err(|_| {
            BridgeError::NotFound(format!("SOLIDCRAFT_CONTROL_PORT `{port}` is not a port"))
        })?;
        return Ok(Endpoint::new(port, &token));
    }
    let directory = control_directory()
        .ok_or_else(|| BridgeError::NotFound("no control folder for this platform".into()))?;
    find_in(&directory, timeout)
}

/// As [`find`], in a given control folder.
pub fn find_in(directory: &Path, timeout: Duration) -> Result<Endpoint, BridgeError> {
    let candidates = candidates(directory);
    if candidates.is_empty() {
        return Err(BridgeError::NotFound(format!(
            "no token in {}: start SolidCraft and turn on Preferences › Assistant › Let AI agents connect",
            directory.display()
        )));
    }
    candidates
        .into_iter()
        .find(|e| TcpStream::connect_timeout(&e.address(), timeout).is_ok())
        .ok_or_else(|| {
            BridgeError::NotFound(format!(
                "the token files in {} belong to apps no longer running: start SolidCraft",
                directory.display()
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "optobench-bridge-discovery-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn the_current_port_comes_first_then_the_newest() {
        let dir = scratch("order");
        std::fs::write(dir.join("40001.token"), "aa\n").expect("written");
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("40002.token"), "bb\n").expect("written");
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(dir.join("40003.token"), "cc\n").expect("written");
        std::fs::write(dir.join("40003.pid"), "1").expect("written");
        std::fs::write(dir.join("notes.txt"), "x").expect("written");
        std::fs::write(dir.join("current.port"), "40001\n").expect("written");
        let ports: Vec<u16> = candidates(&dir).iter().map(|e| e.port).collect();
        assert_eq!(ports, [40001, 40003, 40002]);
        assert_eq!(candidates(&dir)[0].token(), "aa");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_token_is_never_shown() {
        let endpoint = Endpoint::new(27878, "0123456789abcdef");
        let shown = format!("{endpoint:?} {endpoint}");
        assert!(!shown.contains("0123456789abcdef"), "{shown}");
        assert!(shown.contains("27878"));
    }

    #[test]
    fn stale_token_files_find_nothing() {
        // A port nothing listens on: bind one, note it, let it go.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a port")
            .port();
        let dir = scratch("stale");
        std::fs::write(dir.join(format!("{port}.token")), "aa").expect("written");
        let error = find_in(&dir, Duration::from_millis(200)).expect_err("nothing runs");
        assert!(matches!(error, BridgeError::NotFound(_)), "{error}");
        let empty = scratch("empty");
        let error = find_in(&empty, Duration::from_millis(200)).expect_err("no tokens");
        assert!(
            error.to_string().contains("Let AI agents connect"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }
}
