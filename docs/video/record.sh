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
trap 'rm -f "$raw" "$raw"-0.log*' EXIT

cd "$here/../.."
VINERYLAB_TOUR=$tour VINERYLAB_RECORD=$raw cargo run --release --quiet --bin vinerylab

# The recorder's encode is quick and fragmented. This one spends 9 MB, as
# GitHub takes 10 on a free plan: two passes at the bitrate that fills it,
# however long the video. It starts playing before it has loaded.
secs=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$raw")
x264=(-c:v libx264 -preset slow -b:v "$(awk "BEGIN { print int(72000 / $secs) }")k"
    -pix_fmt yuv420p -passlogfile "$raw")
ffmpeg -y -loglevel warning -i "$raw" "${x264[@]}" -pass 1 -f null /dev/null
ffmpeg -y -loglevel warning -i "$raw" "${x264[@]}" -pass 2 -movflags +faststart "$out"
# A frame every two seconds from the first, four to a row.
rows=$(( (${secs%.*} / 2 + 4) / 4 ))
ffmpeg -y -loglevel warning -i "$out" \
    -vf "select='isnan(prev_selected_t)+gte(t-prev_selected_t,2)',scale=480:-2,tile=4x$rows" \
    -frames:v 1 -update 1 "${out%.*}-sheet.png"
echo "$out ($(numfmt --to=si --suffix=B "$(stat -c %s "$out")")), ${out%.*}-sheet.png"
