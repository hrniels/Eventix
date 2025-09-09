#!/usr/bin/env bash

input="data/static/icon.png"
outdir="data/icons"

for size in 32 48 64 128 256; do
    convert "$input" -resize "${size}x${size}" "$outdir/${size}x${size}.png"
done
