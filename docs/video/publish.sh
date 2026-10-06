#!/usr/bin/env bash
# Puts a README's video on GitHub and points the README at it. GitHub plays
# only video it hosts, and a comment is what `gh` can attach one to: the mp4
# goes up in a comment on the pull request, and the URL GitHub gave it
# replaces the README's video line. Check the video first: its skill says how.
#
#   docs/video/publish.sh [video.mp4] [README.md] [pull request]
#
# Defaults to target/video/vinerylab.mp4, the top README.md and the current
# branch's pull request. Needs gh 2.102 or later, for `--attach`, and push
# access. It leaves the README edited, for the commit that changed the video.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
video=$(realpath "${1:-$repo/target/video/vinerylab.mp4}")
readme=$(realpath "${2:-$repo/README.md}")
# The video line: the one line that is an uploaded asset alone, which GitHub
# draws as a player.
line='^https://github[.]com/user-attachments/assets/[0-9a-f-]+$'
[[ $(grep -cE "$line" "$readme") == 1 ]] || { echo "$readme needs one video line" >&2; exit 1; }

# gh reads the repository, and the branch's pull request, from the checkout.
cd "$repo"
pr=${3:-$(gh pr view --json number --jq .number)}
# The reference has to be its paragraph alone to play rather than link. gh
# rewrites it to the upload, and prints the comment's URL.
comment=$(gh pr comment "$pr" --attach "$video" \
    --body "$(printf 'The video for `%s`:\n\n![](%s)\n' "${readme#"$repo"/}" "$video")")
url=$(gh api "repos/{owner}/{repo}/issues/comments/${comment##*-}" --jq .body |
    grep -oE 'https://github\.com/user-attachments/assets/[0-9a-f-]+' | tail -1)
[[ -n $url ]] || { echo "no video in $comment" >&2; exit 1; }
sed -i -E "s|$line|$url|" "$readme"
echo "$url in ${readme#"$repo"/}, from $comment"
