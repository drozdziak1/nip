use super::serde_cbor;

use failure::Error;
use futures::Stream;
use ipfs_api::IpfsClient;
use tokio_core::reactor::Core;

use std::{cmp::Ordering, collections::BTreeSet};

use constants::{NIP_HEADER_LEN, NIP_PROTOCOL_VERSION};
use nip_ref::NIPRef;
use nip_remote::NIPRemote;
use util::{ipns_deref, parse_nip_header};

/// The "entrypoint" data structure for a nip instance traversing a repo
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct NIPIndex {
    /// All branch and tag tips this index understands
    pub refs: BTreeSet<NIPRef>,
    /// The IPFS hash of the previous index
    pub prev_idx_hash: Option<String>,
}

impl NIPIndex {
    pub fn from_nip_remote(remote: &NIPRemote, ipfs: &mut IpfsClient) -> Result<Self, Error> {
        match remote {
            NIPRemote::ExistingIPFS(ref hash) => {
                info!("Fetching NIPIndex from /ipfs/{}", hash);
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
                Ok(idx)
            }
            NIPRemote::ExistingIPNS(ref hash) => {
                Ok(Self::from_nip_remote(&ipns_deref(hash.as_str(), ipfs)?.parse()?, ipfs)?)
            }
            NIPRemote::NewIPFS | NIPRemote::NewIPNS => Ok(NIPIndex {
                refs: BTreeSet::new(),
                prev_idx_hash: None,
            }),
        }
    }
}
