#!/usr/bin/env bash

discover=false
if [ "$1" = "--discover" ]; then
    discover=true
    shift
fi

if $discover; then
    yes | vdirsyncer -c "$HOME/.config/vdirsyncer-test/config" discover
fi
vdirsyncer -c "$HOME/.config/vdirsyncer-test/config" sync "$@"
