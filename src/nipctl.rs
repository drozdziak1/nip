#[macro_use]
extern crate log;

extern crate clap;
extern crate failure;
extern crate git2;
extern crate ipfs_api;
extern crate serde_json;
extern crate tokio;

extern crate nip_core;

use clap::{App, Arg, ArgMatches, SubCommand};
use failure::Error;
use ipfs_api::IpfsClient;
use log::LevelFilter;
use tokio::runtime::Runtime;

use std::{process, str::FromStr};

use nip_core::{
    init_logging, ipfs_cat, migrate_index, migrate_object, parse_nip_header, NIPIndex, NIPRemote,
    NIP_HEADER_LEN, NIP_PROTOCOL_VERSION,
};

pub fn main() {
    init_logging(LevelFilter::Info);

    let cli_matches = App::new("nipctl")
        .version(env!("CARGO_PKG_VERSION"))
        .about("The repo administration utility for nip.")
        .subcommand(
            SubCommand::with_name("list")
            .about("Prints out a nip IPFS/IPNS link of any type human-readably")
            .arg(
                Arg::with_name("ipfs_hash")
                .help("The IPFS/IPNS hash to get the target from")
                .required(true)
                .index(1),
                )
            .arg(
                Arg::with_name("rollback")
                .short("r")
                .long("--rollback")
                .value_name("N")
                .help("When listing an index move at most N steps back using the previous IPFS hash index field; Ignored for objects"),
                )
            .arg(
                Arg::with_name("json")
                .short("j")
                .long("--json")
                .help("List the structure in JSON")
                )
            )
            .get_matches();

    let mut ipfs = IpfsClient::new("localhost", 5001).unwrap_or_else(|e| {
        error!("Could not reach local IPFS instance: {}", e);
        process::exit(1);
    });

    // Test connectivity to IPFS
    let mut event_loop = Runtime::new().unwrap();

    let stats = event_loop
        .block_on(ipfs.stats_repo())
        .map_err(|e| {
            error!("Could not connect to IPFS, are you sure `ipfs daemon` is running?");
            debug!("Raw error: {}", e);
            process::exit(1);
        })
        .unwrap();

    debug!("IPFS connectivity OK. Datastore stats:\n{:#?}", stats);

    match cli_matches.subcommand() {
        ("list", Some(matches)) => {
            let nip_remote: NIPRemote = matches
                .value_of("ipfs_hash")
                .unwrap()
                .replace("nip::", "")
                .replace("nipdev::", "")
                .parse()
                .unwrap_or_else(|e: Error| {
                    error!("{}", e);
                    println!("{}", matches.usage());
                    process::exit(1);
                });

            debug!("Parsed link {}", nip_remote.to_string());

            let ipfs_hash = nip_remote.get_hash().unwrap();
            let bytes = ipfs_cat(&ipfs_hash, &mut ipfs).unwrap();
            let version = parse_nip_header(bytes.as_slice()).unwrap();
            debug!("nip protocol version {}", version);

            if version < NIP_PROTOCOL_VERSION {
                info!(
                    "Migrating {}: version {} -> {}",
                    ipfs_hash, version, NIP_PROTOCOL_VERSION
                );
            }
            match migrate_index(&bytes[NIP_HEADER_LEN..], version, &mut ipfs) {
                Ok(idx) => handle_index(&idx, &nip_remote, matches, &mut ipfs),
                Err(e) => {
                    debug!("Could not treat bytes as index: {}", e.to_string());
                    debug!("trying object parsing");
                    migrate_and_handle_object(bytes.as_slice(), version, &nip_remote, matches);
                }
            }
        }
        _other => {
            error!("No subcommand specified. Run with -h for full usage.");
        }
    }
}

/// A helper that migrates an object and prints it.
#[inline]
fn migrate_and_handle_object(
    bytes: &[u8],
    version: u16,
    nip_remote: &NIPRemote,
    matches: &ArgMatches,
) {
    match migrate_object(&bytes[NIP_HEADER_LEN..], "<unknown>", version) {
        Ok(obj) => {
            debug!("NIPObject at {}:", nip_remote.to_string());
            if matches.is_present("json") {
                println!("{}", serde_json::to_string_pretty(&obj).unwrap());
            } else {
                println!("{:#?}", obj);
            }
        }
        Err(e) => {
            error!("Could not read index/object: {}", e.to_string());
            process::exit(1);
        }
    }
}

/// A helper that prints an index and rolls it back if possible/requested
#[inline]
fn handle_index(
    idx: &NIPIndex,
    nip_remote: &NIPRemote,
    matches: &ArgMatches,
    ipfs: &mut IpfsClient,
) {
    let rollback_count: u32 = matches
        .value_of("rollback")
        .map_or_else(|| Ok(0), |val| val.parse())
        .unwrap_or_else(|e| {
            error!("Could not parse rollback count: {}", e);
            process::exit(1);
        });

    debug!("Requested {} rollback(s).", rollback_count);

    let mut idx = idx.clone();
    let mut current_remote = nip_remote.clone();
    let mut i = 0;
    while let Some(prev_idx_hash) = idx.prev_idx_hash.clone() {
        if i >= rollback_count {
            break;
        }

        let new_remote = NIPRemote::from_str(&prev_idx_hash).unwrap_or_else(|e| {
            error!("Could not parse previous hash {}: {}", prev_idx_hash, e);
            process::exit(1)
        });

        info!(
            "Reverting {} => {}",
            current_remote.to_string(),
            new_remote.to_string()
        );

        current_remote = new_remote;

        let idx_bytes = ipfs_cat(&prev_idx_hash, ipfs).unwrap();
        let version = parse_nip_header(idx_bytes.as_slice()).unwrap();

        if version < NIP_PROTOCOL_VERSION {
            info!(
                "Migrating {}: version {} -> {}",
                prev_idx_hash, version, NIP_PROTOCOL_VERSION
            );
        }
        idx = migrate_index(&idx_bytes[NIP_HEADER_LEN..], version, ipfs).unwrap_or_else(|e| {
            error!("Could not get index {} from IPFS: {}", prev_idx_hash, e);
            process::exit(1);
        });

        i += 1;
    }

    if i < rollback_count {
        warn!(
            "Only {} rollbacks were made ({} requested, current index chain ends at {})",
            i,
            rollback_count,
            current_remote.to_string()
        );
    }
    info!("nip index at {}:", current_remote.to_string());
    if matches.is_present("json") {
        println!("{}", serde_json::to_string_pretty(&idx).unwrap())
    } else {
        println!("{:#?}", idx);
    }
}
