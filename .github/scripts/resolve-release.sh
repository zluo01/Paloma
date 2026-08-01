#!/usr/bin/env bash
set -euo pipefail

prefix="${1:?release tag prefix is required}"
expected_version="${2:-}"

if [[ "$GITHUB_EVENT_NAME" == "workflow_dispatch" ]]; then
    version="${VERSION_INPUT:?release version is required}"
    tag="${prefix}${version}"
else
    tag="$GITHUB_REF_NAME"
    if [[ "$tag" != "$prefix"* ]]; then
        echo "Unexpected release tag: ${tag}"
        exit 1
    fi
    version="${tag#"$prefix"}"
fi

if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$ ]]; then
    echo "Invalid release version: ${version}"
    exit 1
fi

if [[ -n "$expected_version" && "$version" != "$expected_version" ]]; then
    echo "Release version ${version} does not match project version ${expected_version}."
    exit 1
fi

if [[ "$GITHUB_EVENT_NAME" == "workflow_dispatch" ]] &&
   gh api "repos/${GITHUB_REPOSITORY}/git/ref/tags/${tag}" >/dev/null 2>&1; then
    echo "Tag already exists: ${tag}"
    exit 1
fi

echo "tag=${tag}" >> "$GITHUB_OUTPUT"
echo "version=${version}" >> "$GITHUB_OUTPUT"
