 use std::collections::BTreeSet;

use nip_ref::NIPRef;

/// The "entrypoint" data structure for a nip instance traversing a repo
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct NIPIndex {
    /// All branch and tag tips this index understands
    pub refs: BTreeSet<NIPRef>,
    /// The IPFS hash of the previous index
    pub prev_idx_hash: Option<String>,
}
