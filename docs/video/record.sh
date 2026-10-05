#!/usr/bin/env bash
# Records the README video: plays a storyboard in the viewer, frame by frame,
# and writes an mp4 small enough to upload plus a contact sheet to check it by.
# How to change what it shows: .claude/skills/readme-video/SKILL.md.
#
#   docs/video/record.sh [tour.json] [out.mp4]
#
# Defaults to tour.json beside this script and target/video/vinerylab.mp4.
# Needs cargo, ffmpeg and a display: the window opens on the desktop, at the
# storyboard's size, and ignores the pointer while it records.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
tour=$(realpath "${1:-$here/tour.json}")
out=$(realpath -m "${2:-$here/../../target/video/vinerylab.mp4}")
mkdir -p "$(dirname "$out")"
raw=$(mktemp --suffix .mp4)
trap 'rm -f "$raw"' EXIT

cd "$here/../.."
VINERYLAB_TOUR=$tour VINERYLAB_RECORD=$raw cargo run --release --quiet --bin vinerylab
# The recorder's encode is quick and fragmented; this one is the upload's.
"$here/encode.sh" "$raw" "$out"
