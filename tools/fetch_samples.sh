#!/bin/sh
# Fetches the Khronos glTF-Sample-Assets whose licences keep them out of
# this repository, into target/khronos-fetched/, for the opt-in tests
# (`cargo test --test fetched -- --ignored`):
#
#   DamagedHelmet  CC-BY-4.0 and CC-BY-NC-4.0 (theblueturtle_, ctxwing)
#   Sponza         the CRYENGINE agreement (Crytek)
#
# The checkout is pinned to one commit of KhronosGroup/glTF-Sample-Assets.
set -eu
commit=8f492390ddc7a2ae221a1369ef560589cf8c70fb
root=$(cd "$(dirname "$0")/.." && pwd)
dest="$root/target/khronos-fetched"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git -C "$work" init --quiet
git -C "$work" remote add origin https://github.com/KhronosGroup/glTF-Sample-Assets.git
git -C "$work" sparse-checkout set --no-cone /Models/DamagedHelmet/ /Models/Sponza/
git -C "$work" fetch --quiet --depth 1 --filter=blob:none origin "$commit"
git -C "$work" checkout --quiet FETCH_HEAD
mkdir -p "$dest"
rm -rf "$dest/DamagedHelmet" "$dest/Sponza"
cp -R "$work/Models/DamagedHelmet" "$work/Models/Sponza" "$dest/"
echo "fetched into $dest"
