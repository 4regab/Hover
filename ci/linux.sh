#!/usr/bin/env bash
# The Linux half of CI. ci/buildspec-linux.yml runs it on CodeBuild's Ubuntu 22.04 image
# (glibc 2.35, so the packages run on 22.04 and newer): the tests, and on a release the
# .deb and the tarball, which it copies to the CI bucket for ci/release.sh.
set -euo pipefail
cd "$CODEBUILD_SRC_DIR"

fail() { echo "$*" >&2; exit 1; }

# A release: a pushed v* tag (it must match Cargo.toml), or a push to rust-port/phase-0-1
# whose Cargo.toml version has no tag yet (so raising the version is what releases it).
# ci/windows.ps1 makes the same choice. Cargo.toml has CRLF line endings: the CR isn't
# part of the version.
version=$(tr -d '\r' < Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p' | head -n1)
[ -n "$version" ] || fail "Couldn't read the version from Cargo.toml."
ref=${CODEBUILD_WEBHOOK_HEAD_REF:-}
event=${CODEBUILD_WEBHOOK_EVENT:-}
release=false
if [ "$event" = PUSH ] && [[ "$ref" == refs/tags/v* ]]; then
  [ "$ref" = "refs/tags/v$version" ] || fail "The tag is ${ref#refs/tags/} but Cargo.toml says $version."
  release=true
elif [ "$event" = PUSH ] && [ "$ref" = refs/heads/rust-port/phase-0-1 ]; then
  tagged=$(git ls-remote --tags "$CODEBUILD_SOURCE_REPO_URL" "refs/tags/v$version") || fail "Couldn't list the tags."
  [ -n "$tagged" ] || release=true
fi
echo "event '$event', ref '$ref': version $version, release: $release"

# Build: fontconfig, FreeType, ALSA, xkbcommon. Tests: DejaVu (the chat goldens), dbus,
# GNOME Keyring and Python's GLib (the D-Bus tests skip without them), and Mesa's
# software Vulkan (the office renders headless). CodeBuild runs as root.
apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  build-essential pkg-config libfontconfig1-dev libfreetype-dev libasound2-dev \
  libxkbcommon-dev libxkbcommon-x11-dev fonts-dejavu-core \
  dbus gnome-keyring python3-gi mesa-vulkan-drivers

# Cargo's home is the default /root/.cargo, outside the source folder (whose path changes
# every build): the cache (in the buildspec) keeps its registry and git folders by that
# path, and Cargo rebuilds a cached crate whose source path moved. rust-toolchain.toml
# picks the version.
export CARGO_HOME=/root/.cargo
export PATH="$CARGO_HOME/bin:$PATH"
if ! command -v rustup >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none --no-modify-path
fi
# cargo installs rust-toolchain.toml's version the first time it runs.
cargo --version

# The single-instance lock and the D-Bus tests need a private runtime folder.
# mktemp -d makes it 0700.
XDG_RUNTIME_DIR=$(mktemp -d)
export XDG_RUNTIME_DIR
cargo test --release --workspace --no-fail-fast

[ "$release" = true ] || exit 0

# The version is passed in: the Makefile's own reading keeps Cargo.toml's CR.
make package VERSION="$version"
# The packages go to installers/<this batch>/ in the CI bucket, where ci/release.sh
# looks for them.
batch=$(aws codebuild batch-get-builds --ids "$CODEBUILD_BUILD_ID" --query 'builds[0].buildBatchArn' --output text)
[[ "$batch" == arn:* ]] || fail "This build is not part of a batch build, so the release build would not find the packages."
prefix="s3://${HOVER_CI_BUCKET:?}/installers/${batch##*:}/"
aws s3 cp "dist/hover_${version}_amd64.deb" "$prefix"
aws s3 cp "dist/hover-${version}-linux-x86_64.tar.gz" "$prefix"
