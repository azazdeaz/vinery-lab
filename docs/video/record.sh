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

# The recorder's encode is quick and fragmented. This one is slower and
# smaller — GitHub takes 10 MB on a free plan — and starts playing before it
# has loaded.
ffmpeg -y -loglevel warning -i "$raw" -c:v libx264 -preset slow -crf 26 \
    -pix_fmt yuv420p -movflags +faststart "$out"
# A frame every two seconds from the first, four to a row.
secs=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out")
rows=$(( (${secs%.*} / 2 + 4) / 4 ))
ffmpeg -y -loglevel warning -i "$out" \
    -vf "select='isnan(prev_selected_t)+gte(t-prev_selected_t,2)',scale=480:-2,tile=4x$rows" \
    -frames:v 1 -update 1 "${out%.*}-sheet.png"
echo "$out ($(numfmt --to=si --suffix=B "$(stat -c %s "$out")")), ${out%.*}-sheet.png"
