# nip
nip is a git remote helper that'll put your repo's objects on IPFS - i.e.
**Nowhere In Particular**.

# Installation
Like with most Rust packages, the easiest way to install will be
using Cargo:
```shell
$ cargo install nip
```
# Usage
**Important:** Before you try to use nip please make sure that your local IPFS
instance is running on its standard port.

## Pushing an existing repo to a nip remote for the first time
```shell
$ git remote add nip nip::new-ipfs # Use a magic placeholder URL representing a new IPFS repo
$ git push --all nip # Push all refs to a brand new repo
```

## Cloning a repo from nip
```shell
$ git clone nip::/ipfs/QmZq47khma5nP7DjHUPoERhKnfNUPqkr5pVwmS8A6TQSeN some_repo
```

## Repo administration with nipctl (WIP)
nip comes with `nipctl` - a utility for nip repo administration. As for today
Its functionality is very minimal (printing of objects and indices), but some of
the planned features include:
* Garbage collection - for removing all objects not
associated with any `refs` items
* Managing git push notification settings - Depends on
https://github.com/drozdziak1/nip/issues/7

# How does it all work?
See `FAQ.md` for a tour of underlying nip functionality.

# Development
If you'd like to hack on nip, the `dev_bootstrap.sh` script is where you should
start. It symlinks `nipctl` and `git-remote-nip` as `nipdevctl` and
`git-remote-nipdev` in `~/.cargo/bin`, respectively. As a result, `git` will
pick `git-remote-nipdev` for every remote that has a `nipdev::<hash_or_mode>`
address instead of `git-remote-nip`, which enables painless testing during
developing.

# Limitations
* Running times - nip will only work as fast as IPFS lets it.
* Repo pinning and git push notifications - people interested in keeping track
of remote repo's progress have no way of knowing about pushes made to it. See
[this issue](https://github.com/drozdziak1/nip/issues/7) for progress on the
solution.
* Disk space - by design local git objects need to have IPFS counterparts which
are kept in your local IPFS node's data store. In practice this means that
every local object pushed to a nip repo needs to be stored on your disk again
in a form that IPFS understands. **However, nip guarantees object
deduplication for _all_ repos you use with it, which means a given git object
is stored on IPFS only once, no matter the repo it comes from.**
* Object size - nip doesn't know yet how to stream objects into/out of the
local repository and will attempt to load them into RAM, this increases the
memory footprint substantially for repos that posess large objects. Tracked
[here](https://github.com/drozdziak1/nip/issues/8).
