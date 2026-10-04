#!/usr/bin/env bash
# Encodes a recording into a video GitHub takes, and a contact sheet to check
# it by. Both README videos go through it.
#
#   docs/video/encode.sh in.mp4 out.mp4 [filters]
#
# `filters` is an ffmpeg filter chain applied on the way, e.g. a scale. The
# sheet is out-sheet.png beside the video: a frame every two seconds from the
# first, four to a row.
set -euo pipefail
raw=$1 out=$2 filters=${3:-null}
log=$(mktemp)
trap 'rm -f "$log"*' EXIT

# 9 MB, as GitHub takes 10 on a free plan: two passes at the bitrate that
# fills it, however long the video. It starts playing before it has loaded.
secs=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$raw")
x264=(-vf "$filters" -c:v libx264 -preset slow -b:v "$(awk "BEGIN { print int(72000 / $secs) }")k"
    -pix_fmt yuv420p -passlogfile "$log")
ffmpeg -y -loglevel warning -i "$raw" "${x264[@]}" -pass 1 -f null /dev/null
ffmpeg -y -loglevel warning -i "$raw" "${x264[@]}" -pass 2 -movflags +faststart "$out"
rows=$(( (${secs%.*} / 2 + 4) / 4 ))
ffmpeg -y -loglevel warning -i "$out" \
    -vf "select='isnan(prev_selected_t)+gte(t-prev_selected_t,2)',scale=480:-2,tile=4x$rows" \
    -frames:v 1 -update 1 "${out%.*}-sheet.png"
echo "$out ($(numfmt --to=si --suffix=B "$(stat -c %s "$out")")), ${out%.*}-sheet.png"
