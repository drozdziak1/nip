#[macro_use]
extern crate log;

extern crate clap;
extern crate failure;
extern crate git2;
extern crate ipfs_api;
extern crate tokio_core;

extern crate nip_core;

use clap::{App, Arg, SubCommand};
use failure::Error;
use ipfs_api::IpfsClient;
use log::LevelFilter;

use std::process;

use nip_core::{util, NIPIndex, NIPObject, NIPRemote};

pub fn main() {
    util::init_logging(LevelFilter::Info);

    let cli_matches = App::new("nipctl")
        .version(env!("CARGO_PKG_VERSION"))
        .about("The repo administration utility for nip.")
        .subcommand(
            SubCommand::with_name("list")
                .about("Prints out a nip index human-readably")
                .arg(
                    Arg::with_name("ipfs_hash")
                        .help("The IPFS/IPNS hash to get the index from")
                        .required(true)
                        .index(1),
                ),
        )
        .get_matches();

    let mut ipfs = IpfsClient::default();

    match cli_matches.subcommand() {
        ("list", Some(matches)) => {
            let nip_remote: NIPRemote = matches
                .value_of("ipfs_hash")
                .unwrap()
                .parse()
                .unwrap_or_else(|e: Error| {
                    error!("{}", e);
                    println!("{}", matches.usage());
                    process::exit(1);
                });

            debug!("Parsed link {}", nip_remote.to_string());

            match NIPIndex::from_nip_remote(&nip_remote, &mut ipfs) {
                Ok(idx) => {
                    println!("nip index at {}:\n{:#?}", nip_remote.to_string(), idx);
                }
                Err(e) => {
                    debug!(
                        "Identifying {} as an index failed: {}",
                        nip_remote.to_string(),
                        e
                    );
                    match NIPObject::ipfs_get(&nip_remote.to_string(), &mut ipfs) {
                        Ok(obj) => {
                            println!("nip object at {}:\n{:#?}", nip_remote.to_string(), obj);
                        }
                        Err(e) => {
                            error!(
                                "Could not list nip index/object at {}: {}",
                                nip_remote.to_string(), e
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
