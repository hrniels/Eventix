#!/usr/bin/env bash

discover=false
if [ "$1" = "--discover" ]; then
    discover=true
fi

source "$HOME/.config/systemd/user/vdirsyncer.key"
export KEEPASS_PW

if $discover; then
    yes | vdirsyncer discover
fi
vdirsyncer sync
