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
extern crate git2;
extern crate hyper;
extern crate ipfs_api;
extern crate libc;
extern crate serde;
extern crate serde_cbor;
extern crate tokio_core;

mod constants;
mod nip_index;
mod nip_object;
mod nip_remote;
mod util;

use docopt::Docopt;
use failure::Error;
use git2::Repository;
use ipfs_api::IpfsClient;
use log::LevelFilter;
use tokio_core::reactor::Core;

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

/// NIP's remote helper API capabilities
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
    let mut idx = NIPIndex::from_nip_remote(&remote_type, &mut ipfs).unwrap();
    trace!("Using index {:#?}", idx);

    let mut input_handle = BufReader::new(io::stdin());
    let mut output_handle = io::stdout();

    handle_capabilities(&mut input_handle, &mut output_handle).unwrap();
    handle_list(&mut input_handle, &mut output_handle, &remote_type, &idx).unwrap();

    let mut repo = Repository::open_from_env().unwrap();

    handle_fetches_and_pushes(
        &mut input_handle,
        &mut output_handle,
        &mut repo,
        &remote_type,
        &args.arg_remote,
        &mut ipfs,
        &mut idx,
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
            for (name, git_hash) in &idx.refs {
                let mut output = format!("{} {}", git_hash, name);
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
    repo: &mut Repository,
    remote_type: &NIPRemote,
    remote_name: &str,
    ipfs: &mut IpfsClient,
    idx: &mut NIPIndex,
) -> Result<(), Error> {
    for line in input_handle.lines() {
        let line_buf = line?;
        match line_buf.as_str() {
            // fetch <sha> <ref_name>
            fetch_line if fetch_line.starts_with("fetch") => {
                trace!("Raw fetch line {:?}", fetch_line);

                // Skip the "fetch" part
                let mut iter = fetch_line.split_whitespace().skip(1);

                let hash_to_fetch = iter.next().ok_or_else(|| {
                    format_err!(
                        "Could not read in ref git hash from fetch line: {:?}",
                        fetch_line
                    )
                })?;
                debug!("Parsed git hash: {}", hash_to_fetch);

                let target_ref_name = iter.next().ok_or_else(|| {
                    format_err!(
                        "Could not read in ref name from fetch line: {:?}",
                        fetch_line
                    )
                })?;
                debug!("Parsed ref name: {}", target_ref_name);

                idx.fetch_to_ref_from_str(hash_to_fetch, target_ref_name, repo, ipfs)?;
            }
            // push <refspec>
            push_line if push_line.starts_with("push") => {
                trace!("Raw push line {:?}", push_line);

                // Skip the "push" part
                let refspec = push_line.split_whitespace().nth(1).ok_or_else(|| {
                    format_err!("Could not read in refspec from push line: {:?}", push_line)
                })?;

                // Separate source, destination and the force flag
                let mut refspec_iter = refspec.split(':');

                let first_half = refspec_iter.next().ok_or_else(|| {
                    format_err!("Could not read source ref from refspec: {:?}", refspec)
                })?;

                let force = first_half.starts_with('+');

                let src = if force {
                    warn!("THIS PUSH WILL BE FORCED");
                    &first_half[1..]
                } else {
                    first_half
                };
                debug!("Parsed src: {}", src);

                let dst = refspec_iter.next().ok_or_else(|| {
                    format_err!("Could not read destination ref from refspec: {:?}", refspec)
                })?;
                debug!("Parsed dst: {}", dst);

                // Upload the object tree
                idx.push_ref_from_str(src, dst, repo, ipfs)?;
                debug!("Index after upload: {:#?}", idx);

                let new_hash = idx.ipfs_add(ipfs)?;

                let new_remote_type: NIPRemote = match remote_type {
                    NIPRemote::NewIPFS | NIPRemote::ExistingIPFS(..) => new_hash.parse()?,
                    NIPRemote::NewIPNS | NIPRemote::ExistingIPNS(..) => {
                        let mut event_loop = Core::new()?;

                        let publish_req = ipfs.name_publish(&new_hash, true, None, None, None);

                        let ipns_hash = format!("/ipns/{}", event_loop.run(publish_req)?.name);
                        ipns_hash.parse()?
                    }
                };

                let new_url = match &new_remote_type {
                    NIPRemote::NewIPFS | NIPRemote::NewIPNS => {
                        panic!("INTERNAL ERROR: we have just uploaded the index, there's no way for it to be new at this point");
                    }
                    _existing => {
                        trace!("Forming new URL for remote {}", remote_name);
                        if remote_name.ends_with("nipdev") {
                            format!("nipdev::{}", new_remote_type.to_string())
                        } else {
                            format!("nip::{}", new_remote_type.to_string())
                        }
                    }
                };

                info!(
                    "NIP Remote {} moves onto a new hash:\nPrevious: {}\nNew: {}\nFull new repo address: {}",
                    remote_name,
                    remote_type.to_string(),
                    new_remote_type.to_string(),
                    new_url
                    );

                repo.remote_set_url(remote_name, &new_url)?;

                // Tell git we're done with this ref
                writeln!(output_handle, "ok {}", dst)?;
            }
            // The lines() iterator clips the newline by default, so the last line match is ""
            "" => {
                trace!("Consumed all fetch/push commands");
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
