pub static IPFS_HASH_LEN: usize = 46;

// Protocol header components, loosely placed just before serialized NIPIndices to allow for
// backwards compat at all times (65k-entry, 2-byte version space, constant 8-byte width,
// independence from serde)
pub static NIP_MAGIC: &[u8] = b"NIPNIP";
pub static NIP_PROTOCOL_VERSION: u16 = 1; // Bump on breaking data structure changes
pub static NIP_HEADER_LEN: usize = 8;
