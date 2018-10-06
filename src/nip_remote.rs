use failure::Error;
use futures::{Future, Stream};
use ipfs_api::IpfsClient;
use serde_cbor;
use tokio_core::reactor::Core;

use std::{cmp::Ordering, collections::BTreeSet, str::FromStr, string::ToString};

use constants::{IPFS_HASH_LEN, NIP_HEADER_LEN, NIP_PROTOCOL_VERSION};
use nip_ref::NIPRef;
use nip_index::NIPIndex;
use util::parse_nip_header;

#[derive(Clone, Debug, PartialEq)]
/// A representation of a NIP remote repository
pub enum NIPRemote {
    ExistingIPFS(String), // Use a supplied existing repo hash
    ExistingIPNS(String), // Resolve and use an existing IPNS record
    NewIPFS,              // Create a brand new IPFS-hosted NIP repo
    NewIPNS,              // Update local IPNS record. TODO: Support using a specified IPNS key
}

#[derive(Debug, Fail, PartialEq)]
pub enum NIPRemoteParseError {
    #[fail(display = "Got a hash {} chars long, expected {}", _0, _1)]
    InvalidHashLength(usize, usize),
    #[fail(display = "Invalid link format")]
    InvalidLinkFormat,
    #[fail(display = "Failed to parse remote type: {}", _0)]
    Other(String),
}

impl NIPRemote {
    pub fn list_refs(&self, ipfs: &mut IpfsClient) -> Result<BTreeSet<NIPRef>, Error> {
        match self {
            NIPRemote::ExistingIPFS(ref hash) => {
                info!("Fetching /ipfs/{}", hash);
                let mut event_loop = Core::new()?;
                let req = ipfs.cat(hash).concat2();

                let bytes = event_loop.run(req)?;

                match String::from_utf8(bytes.to_vec()) {
                    Ok(s) => trace!("Received string:\n{}", s),
                    Err(_e) => trace!("Received raw bytes:\n{:?}", bytes),
                }

                let protocol_version = parse_nip_header(&bytes[..NIP_HEADER_LEN])?;
                debug!("Index protocol version {}", protocol_version);
                match protocol_version.cmp(&NIP_PROTOCOL_VERSION) {
                    Ordering::Less => debug!(
                        "NIP index is {} protocol versions behind, migrating...",
                        NIP_PROTOCOL_VERSION - protocol_version
                    ),
                    Ordering::Equal => {}
                    Ordering::Greater => {
                        error!(
                            "NIP index is {} protocol versions ahead, please upgrade NIP to use it",
                            protocol_version - NIP_PROTOCOL_VERSION
                        );
                        bail!("Our NIP is too old");
                    }
                }
                let idx: NIPIndex = serde_cbor::from_slice(&bytes[NIP_HEADER_LEN..])?;
                Ok(idx.refs)
            }
            NIPRemote::ExistingIPNS(ref hash) => {
                info!("Hash /ipns/{} comes from IPNS, dereferencing...", hash);
                Ok(BTreeSet::new())
            }
            NIPRemote::NewIPFS | NIPRemote::NewIPNS => {
                warn!("fetch_refs(): Unexpected {} remote", self.to_string());
               Ok(BTreeSet::new())
            }
        }

    }
}

impl FromStr for NIPRemote {
    type Err = Error;
    fn from_str(s: &str) -> Result<NIPRemote, Error> {
        match s {
            "new-ipfs" => Ok(NIPRemote::NewIPFS),
            "new-ipns" => Ok(NIPRemote::NewIPNS),
            existing_ipfs if existing_ipfs.starts_with("/ipfs/") => {
                let hash = existing_ipfs
                    .split('/')
                    .nth(2)
                    .ok_or_else(|| NIPRemoteParseError::Other("Invalid hash format".to_owned()))?;
                if hash.len() != IPFS_HASH_LEN {
                    return Err(
                        NIPRemoteParseError::InvalidHashLength(hash.len(), IPFS_HASH_LEN).into(),
                    );
                }
                Ok(NIPRemote::ExistingIPFS(hash.to_owned()))
            }
            existing_ipns if existing_ipns.starts_with("/ipns/") => {
                let hash = existing_ipns
                    .split('/')
                    .nth(2)
                    .ok_or(NIPRemoteParseError::InvalidLinkFormat)?;
                if hash.len() != IPFS_HASH_LEN {
                    return Err(
                        NIPRemoteParseError::InvalidHashLength(hash.len(), IPFS_HASH_LEN).into(),
                    );
                }
                Ok(NIPRemote::ExistingIPNS(hash.to_owned()))
            }
            _other => Err(NIPRemoteParseError::InvalidLinkFormat.into()),
        }
    }
}

impl ToString for NIPRemote {
    fn to_string(&self) -> String {
        match self {
            NIPRemote::ExistingIPFS(ref hash) => format!("/ipfs/{}", hash),
            NIPRemote::ExistingIPNS(ref hash) => format!("/ipns/{}", hash),
            NIPRemote::NewIPFS => "new-ipfs".to_owned(),
            NIPRemote::NewIPNS => "new-ipns".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parses_new_ipfs() {
        assert_eq!("new-ipfs".parse::<NIPRemote>().unwrap(), NIPRemote::NewIPFS);
    }

    #[test]
    fn test_parses_new_ipns() {
        assert_eq!("new-ipns".parse::<NIPRemote>().unwrap(), NIPRemote::NewIPNS);
    }

    #[test]
    fn test_invalid_link_err() {
        match "gibberish".parse::<NIPRemote>() {
            Err(e) => assert_eq!(
                e.downcast::<NIPRemoteParseError>().unwrap(),
                NIPRemoteParseError::InvalidLinkFormat
            ),
            Ok(_) => panic!("Got an Ok, InvalidLinkFormat"),
        }
    }

    #[test]
    fn test_invalid_hash_len_err() {
        let bs_hash = "/ipfs/QmTooShort";
        match bs_hash.clone().parse::<NIPRemote>() {
            Err(e) => assert_eq!(
                e.downcast::<NIPRemoteParseError>().unwrap(),
                NIPRemoteParseError::InvalidHashLength(bs_hash.len(), IPFS_HASH_LEN)
            ),
            Ok(_) => panic!("Got an Ok, InvalidLinkFormat"),
        }
    }
}
