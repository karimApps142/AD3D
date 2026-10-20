#!/bin/sh
# Downloads the pinned manifold and Clipper2 sources into vendor/ and checks
# their SHA-256. build.rs never touches the network; run this once after a
# fresh checkout (or after bumping a pin). Pins mirror
# [package.metadata.vendor] in Cargo.toml.
set -eu

MANIFOLD_VERSION=3.5.4
MANIFOLD_URL="https://github.com/elalish/manifold/releases/download/v${MANIFOLD_VERSION}/manifold-${MANIFOLD_VERSION}.tar.gz"
MANIFOLD_SHA256=db2a8e7aac6abac12fe54fa7b055d24741362b5706fee6f5c5b8f0bccd2de4ec

# The commit manifold 3.5.4's cmake/manifoldDeps.cmake fetches (GIT_TAG).
CLIPPER2_COMMIT=46f639177fe418f9689e8ddb74f08a870c71f5b4
CLIPPER2_URL="https://github.com/AngusJohnson/Clipper2/archive/${CLIPPER2_COMMIT}.tar.gz"
CLIPPER2_SHA256=73f5783e0c88299976334f48e3e356c756b96212c652ecc8bceec3bd99455bcb

here=$(cd "$(dirname "$0")" && pwd)
vendor="$here/vendor"
dl="$vendor/.download"
mkdir -p "$dl"

fetch() { # name url sha256 dest [subdir-inside-archive]
  name=$1 url=$2 sha=$3 dest=$4 sub=${5:-}
  tgz="$dl/$name.tar.gz"
  if [ ! -f "$tgz" ] || [ "$(shasum -a 256 "$tgz" | cut -d' ' -f1)" != "$sha" ]; then
    echo "downloading $url"
    curl -fsSL --retry 3 -o "$tgz.part" "$url"
    mv "$tgz.part" "$tgz"
  fi
  got=$(shasum -a 256 "$tgz" | cut -d' ' -f1)
  if [ "$got" != "$sha" ]; then
    echo "SHA-256 mismatch for $name: expected $sha, got $got" >&2
    exit 1
  fi
  tmp="$dl/$name.x"
  rm -rf "$tmp" "$dest"
  mkdir -p "$tmp"
  tar -xzf "$tgz" -C "$tmp"
  top=$(ls "$tmp")
  if [ -n "$sub" ]; then mv "$tmp/$top/$sub" "$dest"; else mv "$tmp/$top" "$dest"; fi
  rm -rf "$tmp"
  echo "$name -> $dest (sha256 $sha)"
}

fetch manifold "$MANIFOLD_URL" "$MANIFOLD_SHA256" "$vendor/manifold"
# Whole repo: manifold's FetchContent uses SOURCE_SUBDIR CPP, so
# FETCHCONTENT_SOURCE_DIR_CLIPPER2 must point at the repo root.
fetch clipper2 "$CLIPPER2_URL" "$CLIPPER2_SHA256" "$vendor/clipper2"

# Trim what the build never reads. test/samples/extras are only added with
# MANIFOLD_TEST=ON; python/wasm only with MANIFOLD_PYBIND/JSBIND; manifold
# sets CLIPPER2_TESTS/EXAMPLES/UTILS OFF.
rm -rf "$vendor/manifold/test" "$vendor/manifold/samples" "$vendor/manifold/extras" \
       "$vendor/manifold/docs" "$vendor/manifold/bindings/python" "$vendor/manifold/bindings/wasm"
rm -rf "$vendor/clipper2/CSharp" "$vendor/clipper2/DLL" "$vendor/clipper2/Delphi" \
       "$vendor/clipper2/CPP/Tests" "$vendor/clipper2/CPP/Examples" \
       "$vendor/clipper2/CPP/Utils" "$vendor/clipper2/CPP/BenchMark"

cat > "$vendor/VERSIONS" <<EOF
manifold $MANIFOLD_VERSION $MANIFOLD_URL sha256=$MANIFOLD_SHA256
clipper2 $CLIPPER2_COMMIT $CLIPPER2_URL sha256=$CLIPPER2_SHA256
EOF
echo "ok"
