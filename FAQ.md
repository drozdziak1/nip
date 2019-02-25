# FAQ
## How does git talk to nip?
nip implements what is called a *git remote helper* - a new remote transport
backend that can be used by git for pushes and fetches of remote git
repositories. In fact not so long ago the HTTP transport in git used to
be a separate binary taking advantage of this API. You can read more about the
exact remote helper operation in
[`gitremote-helpers(1)`](https://git-scm.com/docs/git-remote-helpers).

Under the hood, the stuff above means that upon a `git push` to or `git fetch` from
a nip remote, git will run the `git-remote-nip` binary and exchange information
about local/remote states via stdio. Then the binary is expected to carry out a
state sync as per the specification of the push/fetch.

## How does nip interact with git repos and IPFS?
### Local repo
Locally, nip takes advantage of
[`git2-rs`](https://github.com/alexcrichton/git2-rs) which is a set of Rust
bindings to [`libgit2`](https://libgit2.org). `libgit2` is then used to scoop
out or instantiate git objects - depending on whether a `push` or `fetch`
operation is requested by git.

### IPFS storage
For IPFS storage nip uses a fairly thin CBOR-encoded format comprised of two
datatypes: `NIPIndex` and `NIPObject`. `NIPIndex` is what every top-level nip
repo IPFS link points to and effectively the face of every nip remote - it
stores information about all git objects available in a given remote as well as
where branch tips and tags should resolve to. `NIPObject` on the other hand
captures the actual git object tree topology of the repo.

Every `NIPObject` is comprised of three parts:
- An IPFS link to the raw bytes of the underlying  git object - this data isn't
  inlined within the data structure to maximize data deduplication, including
  objects produced by different nip versions or even different IPFS git backends
  that choose to operate in the same manner.
- Its own git hash
- git object-specific metadata - this is done via a helper enum type where the
  variants contain differently arranged git hashes depending on object type:
    - commits - parent hash(es), tree hash
    - trees - children hashes (pointing to another nested tree or a blob)
    - blobs - this variant is purely symbolic, the raw bytes link is sufficient
      since blobs are always leaf nodes in git
    - tag objects - target object hash of the tag; only used for
      annotated/signed tags

### A note on object tree edges in nip
An important fact about `NIPObject` metadata is that the references to other
`NIPObject`s are git hashes and not IPFS ones - it is done that way so that the
Rust code can check if the local git repo already contains a given git object
without making any additional requests to the local IPFS node (it looks them up
in the `NIPIndex` which is always downloaded first).

## How does nip intend to stay backwards-compatible?
Internally, nip prepends every serialized `NIPIndex` and `NIPObject` with a very
simple 8-byte header. It starts with a `b"NIPNIP"` magic followed by a
big-endian 16-bit number denoting the version of the data format a given object
uses. This ensures that even when the serialization format is changed or even if
`serde` is no longer used, `nip` will still be able to find out in time. 
