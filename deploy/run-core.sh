#!/bin/sh
# Pandoc creates temporary files in its working directory. Keep the image root
# read-only and give the process a writable cwd backed by the bounded /tmp tmpfs.
set -eu
runtime=/tmp/md-to-pdf-runtime
mkdir -p "$runtime"
for asset in Rocket.toml static templates themes public; do
  ln -snf "/home/rocket/$asset" "$runtime/$asset"
done
cd "$runtime"
exec md-to-pdf
