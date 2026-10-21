//! The snapshot envelope (kernel-api.md §2): full state bytes plus the format
//! version and content fingerprint. A mismatched format or fingerprint
//! discards the snapshot.

use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub content_fingerprint: u64,
    /// Serialized `State`; the codec is the host's choice.
    pub state: Vec<u8>,
}
