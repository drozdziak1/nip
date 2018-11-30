use super::serde_cbor;

use failure::Error;
use futures::Stream;
use git2::{Object, ObjectType, Oid, Repository};
use ipfs_api::IpfsClient;
use tokio_core::reactor::Core;

use std::{cmp::Ordering, collections::BTreeMap, io::Cursor};

use constants::{NIP_HEADER_LEN, NIP_PROTOCOL_VERSION};
use nip_object::{NIPObject, NIPObjectMetadata};
use nip_remote::NIPRemote;
use util::{gen_nip_header, ipns_deref, parse_nip_header};

/// The "entrypoint" data structure for a nip instance traversing a repo
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct NIPIndex {
    /// All refs this repository knows; a {name -> sha1} mapping
    pub refs: BTreeMap<String, String>,
    /// All objects this repository contains; a {sha1 -> IPFS hash} map
    pub objects: BTreeMap<String, String>,
    /// The IPFS hash of the previous index
    pub prev_idx_hash: Option<String>,
}

impl NIPIndex {
    /// Downlaod from IPFS and instantiate a NIPIndex
    pub fn from_nip_remote(remote: &NIPRemote, ipfs: &mut IpfsClient) -> Result<Self, Error> {
        match remote {
            NIPRemote::ExistingIPFS(ref hash) => {
                debug!("Fetching NIPIndex from /ipfs/{}", hash);
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
            NIPRemote::ExistingIPNS(ref hash) => Ok(Self::from_nip_remote(
                &ipns_deref(hash.as_str(), ipfs)?.parse()?,
                ipfs,
            )?),
            NIPRemote::NewIPFS | NIPRemote::NewIPNS => {
                debug!("Creating new index");
                Ok(NIPIndex {
                    refs: BTreeMap::new(),
                    objects: BTreeMap::new(),
                    prev_idx_hash: None,
                })
            }
        }
    }

    /// Dereference object_ref and add it to IPFS and the index
    pub fn push_ref_from_str(
        &mut self,
        ref_src: &str,
        ref_dst: &str,
        repo: &mut Repository,
        ipfs: &mut IpfsClient,
    ) -> Result<(), Error> {
        let reference = repo.find_reference(ref_src)?;

        let obj = if reference.is_tag() {
            reference.resolve()?.peel(ObjectType::Tag)?
        } else {
            reference.resolve()?.peel(ObjectType::Commit)?
        };

        debug!("{:?} dereferenced to {}", reference.shorthand(), obj.id());
        let _ref_obj_hash = self.push_object_dag(obj.clone(), repo, ipfs)?;
        self.refs
            .insert(ref_dst.to_owned(), format!("{}", obj.id()));
        Ok(())
    }

    /// Check what `reference` is and recursively add it along with any objects it may reference. The
    /// top-level object's IPFS hash is returned.
    pub fn push_object_dag(
        &mut self,
        obj: Object,
        repo: &Repository,
        ipfs: &mut IpfsClient,
    ) -> Result<String, Error> {
        trace!("Current object: {:?} at {}", obj.kind(), obj.id());

        let obj_type = obj.kind().ok_or_else(|| {
            let msg = format!("Cannot determine type of object {}", obj.id());
            error!("{}", msg);
            format_err!("{}", msg)
        })?;

        if self.objects.contains_key(&obj.id().to_string()) {
            trace!("Object already in NIP index: {} {}", obj_type, obj.id());
            return Ok(self.objects[&obj.id().to_string()].clone());
        }

        match obj_type {
            ObjectType::Commit => {
                let commit = obj
                    .as_commit()
                    .ok_or_else(|| format_err!("Could not view {:?} as a commit", obj))?;
                trace!("Handling commit {:?}", commit);

                let tree_obj = obj.peel(ObjectType::Tree)?;
                trace!("Commit {}: Handling tree {}", commit.id(), tree_obj.id());
                // Every commit has a tree
                let _tree_hash = self.push_object_dag(tree_obj, repo, ipfs)?;

                for parent in commit.parents().into_iter() {
                    trace!(
                        "Commit {}: Handling parent commit {}",
                        commit.id(),
                        parent.id()
                    );
                    let _parent_hash = self.push_object_dag(parent.into_object(), repo, ipfs)?;
                }

                let nip_object_hash =
                    NIPObject::from_git_commit(&commit, &repo.odb()?, ipfs)?.ipfs_add(ipfs)?;

                self.objects
                    .insert(format!("{}", obj.id()), nip_object_hash.clone());
                debug!(
                    "Commit {} ({:?}) uploaded to {}",
                    obj.id(),
                    obj_type,
                    nip_object_hash
                );
                return Ok(nip_object_hash);
            }
            ObjectType::Tree => {
                let tree = obj
                    .as_tree()
                    .ok_or_else(|| format_err!("Could not view {:?} as a tree", obj))?;
                debug!("Handling tree {:?}", tree);

                for entry in tree.into_iter() {
                    trace!(
                        "Tree {}: Handling tree entry {} ({:?})",
                        tree.id(),
                        entry.id(),
                        entry.kind()
                    );
                    let _entry_hash = self.push_object_dag(
                        repo.find_object(entry.id(), entry.kind())?,
                        repo,
                        ipfs,
                    )?;
                }

                let nip_object_hash =
                    NIPObject::from_git_tree(&tree, &repo.odb()?, ipfs)?.ipfs_add(ipfs)?;

                self.objects
                    .insert(format!("{}", obj.id()), nip_object_hash.clone());
                debug!(
                    "Tree {} ({:?}) uploaded to {}",
                    obj.id(),
                    obj_type,
                    nip_object_hash
                );
                return Ok(nip_object_hash);
            }
            ObjectType::Blob => {
                let blob = obj
                    .as_blob()
                    .ok_or_else(|| format_err!("Could not view {:?} as a blob", obj))?;
                trace!("Handling blob {:?}", blob);

                let nip_object_hash =
                    NIPObject::from_git_blob(&blob, &repo.odb()?, ipfs)?.ipfs_add(ipfs)?;

                self.objects
                    .insert(format!("{}", obj.id()), nip_object_hash.clone());
                debug!(
                    "Blob {} ({:?}) uploaded to {}",
                    obj.id(),
                    obj_type,
                    nip_object_hash
                );
                return Ok(nip_object_hash);
            }
            ObjectType::Tag => {
                let tag = obj
                    .as_tag()
                    .ok_or_else(|| format_err!("Could not view {:?} as a tag", obj))?;

                let nip_object_hash =
                    NIPObject::from_git_tag(&tag, &repo.odb()?, ipfs)?.ipfs_add(ipfs)?;

                self.objects
                    .insert(format!("{}", obj.id()), nip_object_hash.clone());

                debug!(
                    "Tag {} ({:?}) uploaded to {}",
                    obj.id(),
                    obj_type,
                    nip_object_hash
                );
                return Ok(nip_object_hash);
            }
            other => bail!("Don't know how to traverse a {}", other),
        }
    }

    pub fn fetch_to_ref_from_str(
        &mut self,
        hash_to_fetch: &str,
        target_ref_name: &str,
        repo: &mut Repository,
        ipfs: &mut IpfsClient,
    ) -> Result<(), Error> {
        debug!("Fetching {} for {}", hash_to_fetch, target_ref_name);

        let nip_obj_ipfs_hash = self
            .objects
            .get(hash_to_fetch)
            .ok_or_else(|| {
                let msg = format!("Could not find object {} in the NIP index", hash_to_fetch);
                error!("{}", msg);
                format_err!("{}", msg)
            })?.clone();

        let new_tip_oid = self.fetch_nip_object_dag(&nip_obj_ipfs_hash, repo, ipfs)?;
        if new_tip_oid != Oid::from_str(&hash_to_fetch)? {
            let msg = format!(
                "tip of {} fetch failed: Object tree inconsistency detected; fetched {} from {}, but write result hashes to {}",
                target_ref_name, hash_to_fetch, nip_obj_ipfs_hash, new_tip_oid
                );

            error!("{}", msg);
            bail!("{}", msg);
        }

        match repo.odb()?.read_header(new_tip_oid)?.1 {
            ObjectType::Commit => {
                repo.reference(target_ref_name, new_tip_oid, false, "NIP fetch")?;
            }
            // Somehow git is upset when we set tag refs for it
            ObjectType::Tag => {
                debug!("Not setting ref for tag {}", new_tip_oid);
            }
            other_type => {
                let msg = format!("New tip turned out to be a {} after fetch", other_type);
                error!("{}", msg);
                bail!("{}", msg);
            }
        }

        debug!("Fetched {} for {} OK.", hash_to_fetch, target_ref_name);
        Ok(())
    }

    pub fn fetch_nip_object_dag(
        &mut self,
        nip_obj_ipfs_hash: &str,
        repo: &mut Repository,
        ipfs: &mut IpfsClient,
    ) -> Result<Oid, Error> {
        let nip_obj = NIPObject::ipfs_get(nip_obj_ipfs_hash, ipfs)?;

        trace!(
            "Current NIP object at {}:\n{:#?}",
            nip_obj_ipfs_hash,
            nip_obj,
        );

        match nip_obj.clone().metadata {
            NIPObjectMetadata::Commit {
                parent_git_hashes,
                tree_git_hash,
            } => {
                trace!("Handling NIP commit {}", nip_obj_ipfs_hash);

                let tree_ipfs_hash = self
                    .objects
                    .get(&tree_git_hash)
                    .ok_or_else(|| {
                        let msg =
                            format!("Could not find the IPFS hash for tree {}", tree_git_hash);
                        error!("{}", msg);
                        format_err!("{}", msg)
                    })?.clone();

                trace!(
                    "NIP commit {}: Handling NIP tree at {}",
                    nip_obj_ipfs_hash,
                    tree_ipfs_hash
                );

                let tree_oid = self.fetch_nip_object_dag(&tree_ipfs_hash, repo, ipfs)?;
                if tree_oid != Oid::from_str(&tree_git_hash)? {
                    let msg = format!(
                        "NIP commit {}: Object tree inconsistency detected; fetched {} from {}, but write result hashes to {}",
                        nip_obj_ipfs_hash, tree_git_hash, tree_ipfs_hash, tree_oid);

                    error!("{}", msg);
                    bail!("{}", msg);
                }

                for parent_git_hash in parent_git_hashes {
                    let parent_ipfs_hash = self
                        .objects
                        .get(&parent_git_hash)
                        .ok_or_else(|| {
                            let msg = format!(
                                "NIP commit {}: Could not find the IPFS hash for parent commit {}",
                                nip_obj_ipfs_hash, parent_git_hash
                            );
                            error!("{}", msg);
                            format_err!("{}", msg)
                        })?.clone();

                    trace!(
                        "NIP commit {}: Handling NIP parent commit at {}",
                        nip_obj_ipfs_hash,
                        parent_ipfs_hash
                    );

                    let parent_oid = self.fetch_nip_object_dag(&parent_ipfs_hash, repo, ipfs)?;
                    if parent_oid != Oid::from_str(&parent_git_hash)? {
                        let msg = format!(
                            "Object tree inconsistency detected; fetched {} from {}, but write result hashes to {}",
                            parent_git_hash, nip_obj_ipfs_hash, parent_oid);

                        error!("{}", msg);
                        bail!("{}", msg);
                    }
                }
            }
            NIPObjectMetadata::Tag { target_git_hash } => {
                trace!("Handling NIP tag {}", nip_obj_ipfs_hash);

                let target_ipfs_hash = self
                    .objects
                    .get(&target_git_hash)
                    .ok_or_else(|| {
                        let msg = format!(
                            "Could not find the IPFS hash for target {}",
                            target_git_hash
                        );
                        error!("{}", msg);
                        format_err!("{}", msg)
                    })?.clone();

                trace!(
                    "NIP tag {}: Handling NIP target at {}",
                    nip_obj_ipfs_hash,
                    target_ipfs_hash
                );

                let target_oid = self.fetch_nip_object_dag(&target_ipfs_hash, repo, ipfs)?;
                if target_oid != Oid::from_str(&target_git_hash)? {
                    let msg = format!(
                        "Object tree inconsistency detected; fetched {} from {}, but write result hashes to {}",
                        target_git_hash, target_ipfs_hash, target_oid);

                    error!("{}", msg);
                    bail!("{}", msg);
                }
            }
            NIPObjectMetadata::Tree { entry_git_hashes } => {
                trace!("Handling NIP tree {}", nip_obj_ipfs_hash);

                for entry_git_hash in entry_git_hashes {
                    let tree_entry_ipfs_hash = self
                        .objects
                        .get(&entry_git_hash)
                        .ok_or_else(|| {
                            let msg = format!(
                                "NIP tree {}: Could not find the IPFS hash for entry {}",
                                nip_obj_ipfs_hash, entry_git_hash
                            );
                            error!("{}", msg);
                            format_err!("{}", msg)
                        })?.clone();

                    trace!(
                        "NIP tree {}: Handling NIP tree entry at {}",
                        nip_obj_ipfs_hash,
                        tree_entry_ipfs_hash
                    );

                    let entry_oid = self.fetch_nip_object_dag(&tree_entry_ipfs_hash, repo, ipfs)?;
                    if entry_oid != Oid::from_str(&entry_git_hash)? {
                        let msg = format!(
                            "NIP tree {}: Object tree inconsistency detected; fetched {} from {}, but write result hashes to {}",
                            nip_obj_ipfs_hash, entry_git_hash, nip_obj_ipfs_hash, entry_oid);

                        error!("{}", msg);
                        bail!("{}", msg);
                    }
                }
            }
            NIPObjectMetadata::Blob => {
                trace!("Handling NIP blob {}", nip_obj_ipfs_hash);
            }
        }

        let oid = nip_obj.write_raw_data(&mut repo.odb()?, ipfs)?;
        debug!("Fetched object {} to {}", nip_obj_ipfs_hash, oid);
        Ok(oid)
    }

    pub fn ipfs_add(&mut self, ipfs: &mut IpfsClient) -> Result<String, Error> {
        let mut event_loop = Core::new()?;
        let mut self_buf = gen_nip_header(None)?;

        self_buf.extend_from_slice(&serde_cbor::to_vec(self)?);

        let req = ipfs.add(Cursor::new(self_buf));
        let hash = format!("/ipfs/{}", event_loop.run(req)?.hash);
        self.prev_idx_hash = Some(hash.clone());

        Ok(hash)
    }
}
