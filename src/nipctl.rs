#[macro_use]
extern crate log;

extern crate clap;
extern crate failure;
extern crate git2;
extern crate ipfs_api;
extern crate serde_json;
extern crate tokio_core;

extern crate nip_core;

use clap::{App, Arg, SubCommand};
use failure::Error;
use ipfs_api::IpfsClient;
use log::LevelFilter;

use std::{process, str::FromStr};

use nip_core::{util, NIPIndex, NIPObject, NIPRemote};

pub fn main() {
    util::init_logging(LevelFilter::Info);

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

    let mut ipfs = IpfsClient::default();

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

            match NIPIndex::from_nip_remote(&nip_remote, &mut ipfs) {
                Ok(idx) => {
                    let rollback_count: u32 = matches
                        .value_of("rollback")
                        .map_or_else(|| Ok(0), |val| val.parse())
                        .unwrap_or_else(|e| {
                            error!("Could not parse rollback count: {}", e);
                            process::exit(1);
                        });

                    debug!("Requested {} rollback(s).", rollback_count);

                    let mut idx = idx;
                    let mut current_remote = nip_remote;
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

                        idx = NIPIndex::from_nip_remote(&current_remote, &mut ipfs).unwrap_or_else(
                            |e| {
                                error!("Could not get index {} from IPFS: {}", prev_idx_hash, e);
                                process::exit(1);
                            },
                        );

                        i += 1;
                    }

                    if i < rollback_count {
                        warn!("Only {} rollbacks were made ({} requested, current index chain ends at {})", i, rollback_count, current_remote.to_string());
                    }
                    info!("nip index at {}:", current_remote.to_string());
                    if matches.is_present("json") {
                        println!("{}", serde_json::to_string_pretty(&idx).unwrap())
                    } else {
                        println!("{:#?}", idx);
                    }
                }
                Err(e) => {
                    debug!(
                        "Identifying {} as an index failed: {}",
                        nip_remote.to_string(),
                        e
                    );

                    match NIPObject::ipfs_get(&nip_remote.to_string(), &mut ipfs) {
                        Ok(obj) => {
                            info!("nip object at {}:", nip_remote.to_string());
                            if matches.is_present("json") {
                                println!("{}", serde_json::to_string_pretty(&obj).unwrap())
                            } else {
                                println!("{:#?}", obj);
                            }
                        }
                        Err(e) => {
                            error!(
                                "Could not list nip index/object at {}: {}",
                                nip_remote.to_string(),
                                e
                            );
                            process::exit(1);
                        }
                    }
                }
            }
        }
        _ => unimplemented!(),
    }
}
