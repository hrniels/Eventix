#!/usr/bin/env bash

input="data/icons/scalable.svg"
outdir="data/icons"

for size in 48 64 128 256 512; do
    convert -background none "$input" -resize "${size}x${size}" "$outdir/${size}x${size}.png"
done
