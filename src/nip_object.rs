use failure::Error;
use futures::Stream;
use git2::{Blob, Commit, Odb, OdbObject, Tag, Tree};
use ipfs_api::IpfsClient;
use tokio_core::reactor::Core;

use std::{collections::BTreeSet, io::Cursor};

use constants::{NIP_HEADER_LEN, NIP_PROTOCOL_VERSION};
use util::{gen_nip_header, parse_nip_header};

#[derive(Clone, Deserialize, Serialize)]
pub struct NIPObject {
    pub raw_data_ipfs_hash: String,
    pub metadata: NIPObjectMetadata,
}

#[derive(Clone, Deserialize, Serialize)]
pub enum NIPObjectMetadata {
    Commit {
        parent_git_hashes: BTreeSet<String>,
        tree_git_hash: String,
    },
    Tag {
        target_git_hash: String,
    },
    Tree {
        entry_git_hashes: BTreeSet<String>,
    },
    Blob,
}

impl NIPObject {
    pub fn from_blob(blob: &Blob, odb: &Odb, ipfs: &mut IpfsClient) -> Result<Self, Error> {
        let odb_obj = odb.read(blob.id())?;
        let raw_data_ipfs_hash = Self::upload_odb_obj(odb_obj, ipfs)?;

        Ok(Self {
            raw_data_ipfs_hash,
            metadata: NIPObjectMetadata::Blob,
        })
    }

    pub fn from_commit(commit: &Commit, odb: &Odb, ipfs: &mut IpfsClient) -> Result<Self, Error> {
        let odb_obj = odb.read(commit.id())?;
        let raw_data_ipfs_hash = Self::upload_odb_obj(odb_obj, ipfs)?;
        let parent_git_hashes: BTreeSet<String> = commit
            .parent_ids()
            .map(|parent_id| format!("{}", parent_id))
            .collect();

        let tree_git_hash = format!("{}", commit.tree()?.id());

        Ok(Self {
            raw_data_ipfs_hash,
            metadata: NIPObjectMetadata::Commit {
                parent_git_hashes,
                tree_git_hash,
            },
        })
    }

    pub fn from_tag(tag: &Tag, odb: &Odb, ipfs: &mut IpfsClient) -> Result<Self, Error> {
        let odb_obj = odb.read(tag.id())?;
        let raw_data_ipfs_hash = Self::upload_odb_obj(odb_obj, ipfs)?;

        Ok(Self {
            raw_data_ipfs_hash,
            metadata: NIPObjectMetadata::Tag {
                target_git_hash: format!("{}", tag.target_id())
            },
        })
    }

    pub fn from_tree(tree: &Tree, odb: &Odb, ipfs: &mut IpfsClient) -> Result<Self, Error> {
        let odb_obj = odb.read(tree.id())?;
        let raw_data_ipfs_hash = Self::upload_odb_obj(odb_obj, ipfs)?;

        let entry_git_hashes: BTreeSet<String> =
            tree.iter().map(|entry| format!("{}", entry.id())).collect();

        Ok(Self {
            raw_data_ipfs_hash,
            metadata: NIPObjectMetadata::Tree { entry_git_hashes },
        })
    }

    pub fn ipfs_get(hash: &str, ipfs: &mut IpfsClient) -> Result<Self, Error> {
        let mut event_loop = Core::new()?;

        let object_bytes_req = ipfs.cat(hash).concat2();

        let object_bytes: Vec<u8> = event_loop.run(object_bytes_req)?.into_iter().collect();

        let obj_nip_proto_version = parse_nip_header(&object_bytes)?;

        if obj_nip_proto_version != NIP_PROTOCOL_VERSION {
            bail!(
                "Unsupported protocol version {} (We're at {})",
                obj_nip_proto_version,
                NIP_PROTOCOL_VERSION
                );
        }

        Ok(serde_cbor::from_slice(&object_bytes[NIP_HEADER_LEN..])?)
    }

    fn upload_odb_obj(odb_obj: OdbObject, ipfs: &mut IpfsClient) -> Result<String, Error> {
        let mut event_loop = Core::new()?;

        let obj_buf = odb_obj.data().to_vec();

        let raw_data_req = ipfs.add(Cursor::new(obj_buf));

        Ok(format!("/ipfs/{}", event_loop.run(raw_data_req)?.hash))
    }

    pub fn ipfs_add(&self, ipfs: &mut IpfsClient) -> Result<String, Error> {
        let mut event_loop = Core::new()?;
        let mut self_buf = gen_nip_header(None)?;

        self_buf.extend_from_slice(&serde_cbor::to_vec(self)?);

        let req = ipfs.add(Cursor::new(self_buf));
        let ipfs_hash = format!("/ipfs/{}", event_loop.run(req)?.hash);

        Ok(ipfs_hash)
    }
}
