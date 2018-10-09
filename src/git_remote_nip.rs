#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

extern crate byteorder;
extern crate docopt;
extern crate env_logger;
extern crate futures;
extern crate hyper;
extern crate ipfs_api;
extern crate libc;
extern crate serde;
extern crate serde_cbor;
extern crate tokio_core;

mod constants;
mod nip_index;
mod nip_ref;
mod nip_remote;
mod util;

use docopt::Docopt;
use failure::Error;
use ipfs_api::IpfsClient;
use log::LevelFilter;

use std::{
    env, io,
    io::{BufRead, BufReader, Write},
    process,
};

use nip_index::NIPIndex;
use nip_remote::NIPRemote;

static USAGE: &'static str = "
NIP - the IPFS git remote helper that puts your repo objects Nowhere In Particular.

Usage: git-remote-nip <remote> <mode-or-hash>
       git-remote-nip --help
       git-remote-nip --version
";

static NIP_CAPS: &[&'static str] = &["fetch", "push"];

#[derive(Debug, Deserialize)]
struct NIPArgs {
    arg_remote: String,
    arg_mode_or_hash: String,
}

fn main() {
    util::init_logging(LevelFilter::Info);

    let args: NIPArgs = Docopt::new(USAGE)
        .and_then(|d| {
            d.help(true)
                .version(Some(env!("CARGO_PKG_VERSION").to_owned()))
                .argv(env::args())
                .deserialize()
        }).unwrap_or_else(|e| e.exit());

    trace!("Args: {:#?}", args);

    let remote_type: NIPRemote = args.arg_mode_or_hash.parse().unwrap();

    let mut ipfs = IpfsClient::default();
    let idx = NIPIndex::from_nip_remote(&remote_type, &mut ipfs).unwrap();
    trace!("Obtained index {:#?}", idx);

    let mut input_handle = BufReader::new(io::stdin());
    let mut output_handle = io::stdout();

    handle_capabilities(&mut input_handle, &mut output_handle).unwrap();
    handle_list(&mut input_handle, &mut output_handle, &remote_type, &idx).unwrap();
    handle_fetches_and_pushes(
        &mut input_handle,
        &mut output_handle,
        &remote_type,
        &mut ipfs,
    ).unwrap();
}

fn handle_capabilities(input_handle: &mut BufRead, output_handle: &mut Write) -> Result<(), Error> {
    let mut line_buf = String::new();
    input_handle.read_line(&mut line_buf)?;
    match line_buf.as_str() {
        "capabilities\n" => {
            trace!("Consumed the \"capabilities\" command");
            let mut response = NIP_CAPS.join("\n");
            response.push_str("\n\n");
            output_handle.write_all(response.as_bytes())?;
        }
        other => {
            error!("Received unexpected command {:?}", other);
        }
    }
    Ok(())
}

fn handle_list(
    input_handle: &mut BufRead,
    output_handle: &mut Write,
    remote_type: &NIPRemote,
    idx: &NIPIndex,
) -> Result<(), Error> {
    let mut line_buf = String::new();
    input_handle.read_line(&mut line_buf)?;

    // Consume the command line
    match line_buf.as_str() {
        list if list.starts_with("list") => {
            trace!("Consumed the \"list*\" command");
        }
        // Sometimes git needs to finish early, e.g. when the local ref doesn't even exist locally
        "\n" => {
            debug!("Git finished early, exiting...");
            process::exit(0);
        }
        other => {
            let msg = format!("Expected a \"list*\" command, got {:?}", other);
            error!("{}", msg);
            bail!("{}", msg);
        }
    }

    // Output appropriate response by remote type
    match remote_type {
        // How to proceed with each variant?
        //
        // Pull/Clone: Empty response
        // Push: Upload all refs and index them, return index hash to user
        NIPRemote::NewIPFS | NIPRemote::NewIPNS => {
            debug!("remote_type is new-*, \"list\" response empty");
            output_handle.write_all(b"\n")?;
        }
        // Pull/Clone: Download whatever git wants basing off the index under `hash`
        // Push: Update the ref index and put it on IPFS, return the new hash
        existing => {
            debug!(
                "Listing refs from existing repo at {}",
                existing.to_string()
            );
            debug!("Fetched refs:");

            for nip_ref in &idx.refs {
                let mut output = format!("{} {}", nip_ref.git_hash, nip_ref.name);
                debug!("{}", output);
                writeln!(output_handle, "{}", output)?;
            }

            // Indicate that we're done listing
            writeln!(output_handle)?;
        }
    }
    Ok(())
}

fn handle_fetches_and_pushes(
    input_handle: &mut BufRead,
    output_handle: &mut Write,
    remote_type: &NIPRemote,
    ipfs: &mut IpfsClient,
) -> Result<(), Error> {
    for line in input_handle.lines() {
        let line_buf = line?;
        match line_buf.as_str() {
            fetch_line if fetch_line.starts_with("fetch") => {
                debug!("Raw fetch line {:?}", fetch_line);
            }
            push_line if push_line.starts_with("push") => {
                debug!("Raw push line {:?}", push_line);
            }
            // The lines() iterator clips the newline by default, so the last line match is ""
            "" => {
                debug!("Consumed all \"fetch\" and \"push\" commands");
                break;
            }
            other => {
                let msg = format!(
                    "Git unexpectedly said {:?} during push/fetch parsing.",
                    other
                );
                error!("{}", msg);
                bail!("{}", msg);
            }
        }
    }

    // Tell git that we're done
    writeln!(output_handle)?;

    Ok(())
}
