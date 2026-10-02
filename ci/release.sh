#!/usr/bin/env bash
# The release build. CodeBuild runs it only once the Windows and Linux builds of the same
# batch have passed. Those two decide whether this is a release: on one, each copies its
# installers to installers/<this batch>/ in the CI bucket. Finding none means tests only.
# On a release this tags the commit v<version> and publishes a GitHub pre-release with
# them. GH_TOKEN comes from Secrets Manager (ci/buildspec-release.yml); it is never printed.
set -euo pipefail
cd "$CODEBUILD_SRC_DIR"

fail() { echo "$*" >&2; exit 1; }

# Cargo.toml has CRLF line endings: the CR isn't part of the version.
version=$(tr -d '\r' < Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p' | head -n1)
[ -n "$version" ] || fail "Couldn't read the version from Cargo.toml."

batch=$(aws codebuild batch-get-builds --ids "$CODEBUILD_BUILD_ID" --query 'builds[0].buildBatchArn' --output text)
[[ "$batch" == arn:* ]] || fail "This build is not part of a batch build."
mkdir -p dist
aws s3 cp --recursive "s3://${HOVER_CI_BUCKET:?}/installers/${batch##*:}/" dist/

files=("dist/Hover-Setup-$version.exe" "dist/hover_${version}_amd64.deb" "dist/hover-$version-linux-x86_64.tar.gz")
found=0
for f in "${files[@]}"; do
  if [ -f "$f" ]; then found=$((found + 1)); fi
done
if [ "$found" -eq 0 ]; then
  echo "Not a release: tests only."
  exit 0
fi
# Only if a tag v<version> appeared while the two builds ran, so they chose differently.
[ "$found" -eq 3 ] || fail "Only $found of the 3 installers were built. Nothing was published."

if ! command -v gh >/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSfL https://github.com/cli/cli/releases/download/v2.87.3/gh_2.87.3_linux_amd64.tar.gz | tar -xz -C /tmp
  export PATH="/tmp/gh_2.87.3_linux_amd64/bin:$PATH"
fi

repo=${CODEBUILD_SOURCE_REPO_URL#https://github.com/}
repo=${repo%.git}
tag="v$version"
# A retry of this build finds the release it made before and replaces its files.
if gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  gh release upload "$tag" "${files[@]}" --repo "$repo" --clobber
else
  # The tag is made here when a push released (on a tag push it is that tag). A
  # pre-release is never "Latest": 2.x downloads stay where they are.
  gh release create "$tag" "${files[@]}" --repo "$repo" \
    --target "$CODEBUILD_RESOLVED_SOURCE_VERSION" \
    --title "Hover $tag (native, pre-release)" \
    --prerelease --latest=false --generate-notes
fi
echo "Published $tag."
