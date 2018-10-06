use constants::{GIT_HASH_LEN, IPFS_HASH_LEN};
use failure::Error;

/// A type to represent a git ref stored on IPFS
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialOrd, Ord, PartialEq, Serialize)]
pub struct NIPRef {
    /// e.g. "refs/heads/master"
    pub name: String,
    /// e.g. "5dac866c19d446f356f4a594f66f3a51f3093e65"
    pub git_hash: String,
    /// e.g. "QmfRdhy8MuEZx1XrNfKnTSsMJMu5HsxZFxYTDphjgJngqU"
    pub ipfs_hash: String,
}

impl NIPRef {
    pub fn new(name: String, git_hash: String, ipfs_hash: String) -> Result<Self, Error> {
        let new_self = Self {name, git_hash, ipfs_hash};
        new_self.validate()?;
        Ok(new_self)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.name.is_empty() {
            bail!("Ref name is empty");
        }
        if self.git_hash.len() != GIT_HASH_LEN {
            bail!("Invalid git hash length!");
        }
        if self.ipfs_hash.len() != IPFS_HASH_LEN {
            bail!("Invalid IPFS hash length!");
        }
        Ok(())
    }
}
