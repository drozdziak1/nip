# v0.3.0
What's new:
* Format migrations - applied implicitly in both `git-remote-nip` and `nipctl`
* nip URLs are now colored when different

Breaking changes:
* NIP protocol version 2:
  - Objects contain their git hashes
  - Submodules are denoted with a "submodule-tip" string in `objs`

# v0.2.0
What's new:
* Big repos don't exceed descriptor limits anymore
* Add an IPFS-not-running message

Breaking changes:
* The message about IPFS not running is different

# v0.1.0
Initial release, can push/fetch/pull/clone almost anything that doesn't contain
submodules
