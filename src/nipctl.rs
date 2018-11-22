extern crate env_logger;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

extern crate byteorder;
extern crate futures;
extern crate git2;
extern crate hyper;
extern crate ipfs_api;
extern crate serde_cbor;
extern crate tokio_core;

mod constants;
mod nip_index;
mod nip_object;
mod nip_remote;
mod util;

use ipfs_api::IpfsClient;
use log::LevelFilter;
use tokio_core::reactor::Core;

use std::{collections::BTreeMap, io::Cursor};

use nip_index::NIPIndex;
use util::{gen_nip_header, ipns_deref};

/// A simple binary for managing nip remotes
pub fn main() {
    util::init_logging(LevelFilter::Info);

    info!("Generating a new garbage index");

    let mut buf = gen_nip_header(None).unwrap();

    info!("Header: {:?}", buf.clone());
    let mut refs = BTreeMap::new();
    refs.insert(
        "refs/heads/master".to_owned(),
        "529885ae94597ffdc9c8adae9b643f103c590b88".to_owned(),
    );
    let mut objects = BTreeMap::new();

    let idx = NIPIndex {
        refs,
        objects,
        prev_idx_hash: None,
    };

    buf.extend_from_slice(&serde_cbor::to_vec(&idx).unwrap());
    drop(idx);
    info!("Full serialized bytefield: {:?}", buf.clone());

    let mut ipfs = IpfsClient::default();

    let req = ipfs.add(Cursor::new(buf));
    let mut event_loop = Core::new().unwrap();
    let response = event_loop.run(req).unwrap();
    info!("Response: {:?}", response);

    let publish_req = ipfs.name_publish(response.hash.as_str(), true, None, None, None);
    let published = event_loop.run(publish_req).unwrap();

    info!(
        "Published: /ipns/{}, value: {}",
        published.name, published.value
    );
}
