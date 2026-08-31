#!/usr/bin/env bash
set -Eeuo pipefail

if ! command -v dx >/dev/null 2>&1; then
    echo "Dioxus CLI is required: cargo install dioxus-cli --version 0.7.10 --locked" >&2
    exit 1
fi

# Dioxus serializes the complete rustc environment under target/dx/.captured-args.
# Keep credentials out of those replay files without weakening the build environment.
while IFS= read -r variable_name; do
    case "$variable_name" in
        *_TOKEN | *_SECRET | *_PASSWORD | *_PASSWD | *_PRIVATE_KEY | *_ACCESS_KEY | \
            *_CREDENTIAL | *_CREDENTIALS | AWS_* | AZURE_* | GITHUB_* | GITLAB_* | HF_*)
            unset "$variable_name"
            ;;
    esac
done < <(compgen -e)

exec dx "$@"