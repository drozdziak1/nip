use failure::Error;

use std::{str::FromStr, string::ToString};

use constants::IPFS_HASH_LEN;

#[derive(Clone, Debug, PartialEq)]
/// A representation of a NIP remote repository
pub enum NIPRemote {
    ExistingIPFS(String), // Use a supplied existing repo hash
    ExistingIPNS(String), // Resolve and use an existing IPNS record
    NewIPFS,              // Create a brand new IPFS-hosted NIP repo
    NewIPNS,              // Update local IPNS record. TODO: Support using a specified IPNS key
}

#[derive(Debug, Fail, PartialEq)]
pub enum NIPRemoteParseError {
    #[fail(display = "Got a hash {} chars long, expected {}", _0, _1)]
    InvalidHashLength(usize, usize),
    #[fail(display = "Invalid link format for string \"{}\"", _0)]
    InvalidLinkFormat(String),
    #[fail(display = "Failed to parse remote type: {}", _0)]
    Other(String),
}

impl FromStr for NIPRemote {
    type Err = Error;
    fn from_str(s: &str) -> Result<NIPRemote, Error> {
        match s {
            "new-ipfs" => Ok(NIPRemote::NewIPFS),
            "new-ipns" => Ok(NIPRemote::NewIPNS),
            existing_ipfs if existing_ipfs.starts_with("/ipfs/") => {
                let hash = existing_ipfs
                    .split('/')
                    .nth(2)
                    .ok_or_else(|| NIPRemoteParseError::Other("Invalid hash format".to_owned()))?;
                if hash.len() != IPFS_HASH_LEN {
                    return Err(
                        NIPRemoteParseError::InvalidHashLength(hash.len(), IPFS_HASH_LEN).into(),
                    );
                }
                Ok(NIPRemote::ExistingIPFS(hash.to_owned()))
            }
            existing_ipns if existing_ipns.starts_with("/ipns/") => {
                let hash = existing_ipns.split('/').nth(2).ok_or(
                    NIPRemoteParseError::InvalidLinkFormat(existing_ipns.to_owned()),
                )?;
                if hash.len() != IPFS_HASH_LEN {
                    return Err(
                        NIPRemoteParseError::InvalidHashLength(hash.len(), IPFS_HASH_LEN).into(),
                    );
                }
                Ok(NIPRemote::ExistingIPNS(hash.to_owned()))
            }
            other => Err(NIPRemoteParseError::InvalidLinkFormat(other.to_owned()).into()),
        }
    }
}

impl ToString for NIPRemote {
    fn to_string(&self) -> String {
        match self {
            NIPRemote::ExistingIPFS(ref hash) => format!("/ipfs/{}", hash),
            NIPRemote::ExistingIPNS(ref hash) => format!("/ipns/{}", hash),
            NIPRemote::NewIPFS => "new-ipfs".to_owned(),
            NIPRemote::NewIPNS => "new-ipns".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parses_new_ipfs() {
        assert_eq!("new-ipfs".parse::<NIPRemote>().unwrap(), NIPRemote::NewIPFS);
    }

    #[test]
    fn test_parses_new_ipns() {
        assert_eq!("new-ipns".parse::<NIPRemote>().unwrap(), NIPRemote::NewIPNS);
    }

    #[test]
    fn test_invalid_link_err() {
        match "gibberish".parse::<NIPRemote>() {
            Err(e) => assert_eq!(
                e.downcast::<NIPRemoteParseError>().unwrap(),
                NIPRemoteParseError::InvalidLinkFormat("gibberish".to_owned())
            ),
            Ok(_) => panic!("Got an Ok, InvalidLinkFormat expected"),
        }
    }

    #[test]
    fn test_invalid_hash_len_err() {
        let bs_hash = "/ipfs/QmTooShort";
        match bs_hash.clone().parse::<NIPRemote>() {
            Err(e) => assert_eq!(
                e.downcast::<NIPRemoteParseError>().unwrap(),
                NIPRemoteParseError::InvalidHashLength(
                    bs_hash.len() - 6, // invalid hash len applies to the Qm* part only
                    IPFS_HASH_LEN
                )
            ),
            Ok(_) => panic!("Got an Ok, InvalidHashLength expected"),
        }
    }
}
