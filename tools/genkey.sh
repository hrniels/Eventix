#!/usr/bin/env bash
stty -echo
echo "Enter password:" >&2
read -r pw
stty echo

pw=$(echo "$pw" | \
        systemd-creds encrypt --user --pretty --name=keepass - - | \
        tail -n +2 | \
        sed -Ee 's/\s*(\S*).*/\1/g')
echo "KEEPASS_PW='$pw'"
