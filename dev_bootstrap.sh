#!/bin/sh
build_type=${1:-debug}

set -x
ln -sf $PWD/target/$build_type/git-remote-nip $HOME/.cargo/bin/git-remote-nipdev
ln -sf $PWD/target/$build_type/nipctl $HOME/.cargo/bin/nipdevctl
