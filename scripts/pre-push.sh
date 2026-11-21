#!/usr/bin/env bash
# The pre-push hook `just hooks` installs (docs/development.md §2.9): before
# a push leaves, it runs what the `CI` status check will run on it, so that
# a commit pushed straight to `main` is checked before it lands rather than
# after.
#
# - A push that changes code runs `just check`, the CI gate.
# - A push limited to documentation, Dockerfiles, Compose files or workflows
#   runs `just guards` alone, which is what those files need (AGENTS.md, Git
#   and delivery) and takes seconds.
# - A push that changes nothing (a tag, a deleted branch) runs nothing.
#
# Either way an edited specification must carry a new `generated.at`: the
# guard compares with what the remote already has (OXIDGENE_SPEC_BASE).
#
# The checks read the working tree, not the commits pushed: commit or set
# aside what is not part of the push first. `git push --no-verify` skips the
# hook.
#
# Git gives the remote's name and address as arguments, and on standard
# input one line per ref pushed: local ref, local commit, remote ref, remote
# commit.
set -euo pipefail

remote="${1:-origin}"
cd "$(git rev-parse --show-toplevel)"

# The files AGENTS.md exempts from `just check`.
light='^(docs/|.*\.md$|docker/Dockerfile[^/]*$|docker/docker-compose[^/]*$|\.github/(workflows|actions)/)'

gate=none
base=""
while read -r _ local_sha _ remote_sha; do
    # A deleted ref pushes nothing.
    if [[ "$local_sha" =~ ^0+$ ]]; then
        continue
    fi
    if [[ "$remote_sha" =~ ^0+$ ]] || ! git cat-file -e "$remote_sha^{commit}" 2>/dev/null; then
        # A new branch, or a remote commit this clone has not fetched: what
        # the remote's main branch lacks.
        from=$(git merge-base "$local_sha" "refs/remotes/$remote/main" 2>/dev/null || true)
    else
        from=$remote_sha
    fi
    if [ -z "$from" ]; then
        gate=check
        continue
    fi
    files=$(git diff --name-only "$from" "$local_sha")
    if [ -z "$files" ]; then
        continue
    fi
    base=${base:-$from}
    if grep -qvE "$light" <<<"$files"; then
        gate=check
    elif [ "$gate" = none ]; then
        gate=guards
    fi
done

case "$gate" in
    none)
        exit 0
        ;;
    guards)
        echo "pre-push: documentation, container or workflow files only — running the repository guards (just guards)" >&2
        recipe=guards
        ;;
    check)
        echo "pre-push: running the CI gate (just check); \`git push --no-verify\` skips it" >&2
        recipe=check
        ;;
esac

if [ -n "$base" ]; then
    export OXIDGENE_SPEC_BASE="$base"
fi
# The hook's standard input is the list of refs: the recipes read none.
exec just "$recipe" </dev/null
