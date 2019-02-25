#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

use colored::*;
use docopt::Docopt;
use failure::Error;
use git2::Repository;
use ipfs_api::IpfsClient;
use log::LevelFilter;
use tokio::runtime::current_thread;

use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    process,
};

use nip_core::{ipfs_cat, migrate_index, parse_nip_header, NIPIndex, NIPRemote, NIP_HEADER_LEN};

static USAGE: &'static str = "
nip - the IPFS git remote helper that puts your repo objects Nowhere In Particular.

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
    nip_core::init_logging(LevelFilter::Info);

    let args: NIPArgs = Docopt::new(USAGE)
        .and_then(|d| {
            d.help(true)
                .version(Some(env!("CARGO_PKG_VERSION").to_owned()))
                .argv(env::args())
                .deserialize()
        })
        .unwrap_or_else(|e| e.exit());

    trace!("Args: {:#?}", args);

    let nip_remote: NIPRemote = args.arg_mode_or_hash.parse().unwrap();

    let mut ipfs = IpfsClient::new("localhost", 5001).unwrap_or_else(|e| {
        error!("Could not reach local IPFS instance: {}", e);
        process::exit(1);
    });

    let stats = current_thread::block_on_all(ipfs.stats_repo())
        .map_err(|e| {
            error!("Could not connect to IPFS, are you sure `ipfs daemon` is running?");
            debug!("Raw error: {}", e);
            process::exit(1);
        })
        .unwrap();

    debug!("IPFS connectivity OK. Datastore stats:\n{:#?}", stats);

    let mut idx = if let Some(ipfs_hash) = nip_remote.get_hash() {
        let idx_bytes = ipfs_cat(&ipfs_hash, &mut ipfs).unwrap_or_else(|e| {
            error!("Could not fetch index: {}", e);
            process::exit(1);
        });

        let version = parse_nip_header(idx_bytes.as_slice()).unwrap();

        match migrate_index(&idx_bytes[NIP_HEADER_LEN..], version, &mut ipfs) {
            Ok(idx) => idx,
            Err(e) => {
                error!("Could not parse index: {}", e.to_string());
                process::exit(1);
            }
        }
    } else {
        debug!("Creating a fresh index");
        NIPIndex::from_nip_remote(&nip_remote, &mut ipfs).unwrap()
    };

    trace!("Using index {:#?}", idx);

    let mut input_handle = BufReader::new(io::stdin());
    let mut output_handle = io::stdout();

    handle_capabilities(&mut input_handle, &mut output_handle).unwrap();
    handle_list(&mut input_handle, &mut output_handle, &nip_remote, &idx).unwrap();

    let mut repo = Repository::open_from_env().unwrap();

    handle_fetches_and_pushes(
        &mut input_handle,
        &mut output_handle,
        &mut repo,
        &nip_remote,
        &args.arg_remote,
        &mut ipfs,
        &mut idx,
    )
    .unwrap();
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
    nip_remote: &NIPRemote,
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
    match nip_remote {
        NIPRemote::NewIPFS | NIPRemote::NewIPNS => {
            debug!("remote is new-*, \"list\" response empty");
            output_handle.write_all(b"\n")?;
        }
        existing => {
            debug!(
                "Listing refs from existing repo at {}",
                existing.to_string()
            );
            for (name, git_hash) in &idx.refs {
                let output = format!("{} {}", git_hash, name);
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
    nip_remote: &NIPRemote,
    remote_name: &str,
    ipfs: &mut IpfsClient,
    idx: &mut NIPIndex,
) -> Result<(), Error> {
    let mut current_idx = idx.clone();

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

                current_idx.fetch_to_ref_from_str(hash_to_fetch, target_ref_name, repo, ipfs)?;
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
                match current_idx.push_ref_from_str(src, dst, force, repo, ipfs) {
                    Ok(_) => {}
                    Err(e) => {
                        writeln!(output_handle, "error {} \"{}\"", dst, e)?;
                        continue;
                    }
                }
                debug!("Index after push: {:#?}", current_idx);

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

    // Upload current_idx to IPFS if it differs from the original idx
    match current_idx {
        ref unchanged_idx if unchanged_idx == idx => {
            info!(
                "Current URL: {} (not changed)",
                repo.find_remote(remote_name)?
                    .url()
                    .ok_or_else(|| {
                        let msg = format!("Could not get URL for remote {}", remote_name);
                        error!("{}", msg);
                        format_err!("{}", msg)
                    })?
                    .to_owned()
            );
        }
        mut changed_idx => {
            // Upload the changed index
            let new_nip_remote = changed_idx.ipfs_add(ipfs, Some(nip_remote))?;

            match &new_nip_remote {
                NIPRemote::NewIPFS | NIPRemote::NewIPNS => {
                    bail!("INTERNAL ERROR: we have just uploaded the index, there's no way for it to be new at this point");
                }
                existing => {
                    trace!("Forming new URL for remote {}", remote_name);
                    let current_remote_url = repo
                        .find_remote(remote_name)?
                        .url()
                        .ok_or_else(|| {
                            let msg = format!("Could not get URL for remote {}", remote_name);
                            error!("{}", msg);
                            format_err!("{}", msg)
                        })?
                        .to_owned();

                    trace!("Previous full URL is {}", current_remote_url);

                    let new_repo_url = match current_remote_url {
                        ref _nipdev if _nipdev.starts_with("nipdev") => {
                            info!("nipdev detected!");
                            format!("nipdev::{}", existing.get_hash().unwrap())
                        }
                        ref _nip if _nip.starts_with("nip") => {
                            format!("nip::{}", existing.get_hash().unwrap())
                        }
                        other => {
                            let msg = format!(
                                "Remote {}: URL {} has an unknown prefix",
                                remote_name, other
                            );
                            error!("{}", msg);
                            bail!("{}", msg);
                        }
                    };
                    debug!("Previous IPFS hash: {}", existing.get_hash().unwrap());
                    debug!("New IPFS hash:      {}", existing.get_hash().unwrap());
                    info!("{} {}", "URL changed:".yellow(), new_repo_url.green());

                    repo.remote_set_url(remote_name, &new_repo_url)?;
                }
            };
        }
    }
    // Tell git that we're done
    writeln!(output_handle)?;

    Ok(())
}
