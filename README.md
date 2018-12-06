# nip
nip is a git remote helper that'll put your repo's objects on IPFS - i.e.
**Nowhere In Particular**.

# Installation
The `nip` package is not listed on crates.io yet, but like with many Rust
packages, in due time the easiest way to install will be using Cargo:
```shell
$ cargo install nip # Doesn't work yet!
```
# Usage
**Important:** Before you try to use nip please make sure that your local IPFS
instance is running on its standard port.

## Pushing an existing repo to a nip remote for the first time
```shell
$ git remote add nip nip::new-ipfs # Add a magic remote URL for a new IPFS repo
$ git push --all nip # Push all refs to a brand new repo
 INFO 2018-12-01T15:13:00Z: git_remote_nip: nip Remote nip moves onto a new
hash:
Previous: new-ipfs
New: /ipfs/QmYn3tWpBKaTMgHY8F1cqDXwkHd5TGMqFpvX9ALqhD7Hew
Full new repo address:
nip::/ipfs/QmYn3tWpBKaTMgHY8F1cqDXwkHd5TGMqFpvX9ALqhD7Hew
To nip::/ipfs/QmZq47khma5nP7DjHUPoERhKnfNUPqkr5pVwmS8A6TQSeN
1112e8f..aa89007  master -> master
```

## Cloning a repo from nip
```shell
$ git clone nip::/ipfs/QmZq47khma5nP7DjHUPoERhKnfNUPqkr5pVwmS8A6TQSeN some_repo
Cloning into 'some_repo'...
$ ls some_repo
some_file.txt  some_other_file.txt
$ git log
commit 1112e8f58a9cfbe3a2cfba83ef302357dea266e7 (HEAD -> master, tag: some-new-tag, origin/master)
Author: Stan Drozd <not-showing-my-email@because-spambots.com>
Date:   Tue Nov 27 20:55:40 2018 +0100

   Committing some_other_file.txt

commit 99e9fc231cca0c2f8f70d3c5bac0170a2dfedabe
Author: Stan Drozd <not-showing-my-email@because-spambots.com>
Date:   Tue Nov 27 20:55:40 2018 +0100

   Committing some_file.txt
```

## Repo administration with nipctl (WIP)
nip comes with `nipctl` - a utility for nip repo administration. It's nowhere
near ready yet, but you can view the list of planned features
[here](https://github.com/drozdziak1/nip/issues/6). Suggestions for additional
features are very welcome.

# How it all works a.k.a. FAQ
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

Every `NIPObject` is comprised of two parts:
- An IPFS link to the raw bytes of the underlying  git object - this data isn't
  inlined within the data structure to maximize data deduplication, including
  objects produced by different nip versions or even different IPFS git backends
  that choose to operate in the same manner.
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
in the `NIPIndex` which is always downloaded first). Also, this practice makes
the format less forgiving and therefore less prone to being incorrectly used.

## How does nip intend to stay backwards-compatible?
Internally, nip prepends every serialized `NIPIndex` and `NIPObject` with a very
simple 8-byte header. It starts with a `b"NIPNIP"` magic followed by a
big-endian 16-bit number denoting the version of the data format a given object
uses. This ensures that even when the serialization format is changed or even if
`serde` is no longer used, `nip` will still be able to find out in time.

# Development
If you'd like to hack on nip, the `dev_bootstrap.sh` script is where you should
start.  It symlinks `nipctl` and `git-remote-nip` as `nipdevctl` and
`git-remote-nipdev` in `~/.cargo/bin`, respectively. As a result, `git` will
pick `git-remote-nipdev` for every remote that has a `nipdev::<hash_or_mode>`
address.

# Limitations
* Repo pinning and git push notifications - people interested in keeping track
of remote repo's progress have no way of knowing about pushes made to
different nip repos. See [this
issue](https://github.com/drozdziak1/nip/issues/7) for progress on the solution.
* Submodules - nip doesn't understand how to push/pull submodule pins yet.
* Disk space - by design local git objects need to have IPFS counterparts which
  are kept in your local IPFS node's data store. In practice this means that
  every local object pushed to a nip repo needs to be stored on your disk again
  in a form that IPFS understands. **However, nip guarantees object
  deduplication for _all_ repos you use with it, which greatly reduces the
  problem e.g. when you're working on different forks of the same project**
