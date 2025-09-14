#!/usr/bin/env bash

input="data/icons/scalable.svg"
outdir="data/icons"

for size in 32 48 64 128 256; do
    convert -background none "$input" -resize "${size}x${size}" "$outdir/${size}x${size}.png"
done
convert -background none "$input" -resize "512x512" "data/static/icon.png"
