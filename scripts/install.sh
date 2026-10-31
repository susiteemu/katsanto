#!/bin/sh
# Downloads the latest katsanto release and unpacks the server binary into
# the given directory (default: current directory).
#
#   curl -fsSL https://raw.githubusercontent.com/susiteemu/katsanto/main/scripts/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/susiteemu/katsanto/main/scripts/install.sh | sh -s -- ~/bin
set -eu

repo=susiteemu/katsanto
dest=${1:-.}

# /releases/latest redirects to /releases/tag/<tag>; reading the redirect avoids the API rate limit.
tag=$(curl -fsSI -o /dev/null -w '%{redirect_url}' "https://github.com/$repo/releases/latest")
tag=${tag##*/}
[ -n "$tag" ] || { echo "could not find the latest release" >&2; exit 1; }

name=katsanto-${tag#v}-linux-x86_64
echo "Downloading $name"
mkdir -p "$dest"
curl -fsSL "https://github.com/$repo/releases/download/$tag/$name.tar.gz" |
  tar -xz -C "$dest" --strip-components=1 "$name/katsanto-server"
echo "Installed $dest/katsanto-server ($tag)"
