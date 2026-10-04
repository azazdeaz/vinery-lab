#!/usr/bin/env bash
# Records the pruning demo's video: films a vine pruned in Isaac Sim, headless,
# and writes an mp4 small enough to upload, its captions burned in, plus a
# contact sheet to check it by. How to change what it shows:
# .claude/skills/pruning-video/SKILL.md.
#
#   examples/pruning_demo/record.sh [out.mp4]
#
# Defaults to target/video/pruning.mp4 at the repo root. Needs uv, ffmpeg and
# an RTX GPU, but no display.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=$(realpath -m "${1:-$here/../../target/video/pruning.mp4}")
mkdir -p "$(dirname "$out")"
raw=$(mktemp --suffix .mp4)
trap 'rm -f "$raw" "${raw%.mp4}.srt"' EXIT

cd "$here"
# A run in which some take's cut was not made fails (see the end of
# video.py), and the next one bends the canes a little differently.
for attempt in 1 2 3 4 5; do
    uv run video.py "$raw" && break
    (( attempt < 5 )) || exit 1
    echo "record.sh: recording again, attempt $((attempt + 1)) of 5" >&2
done
# Down to the README video's size, the captions on a box in the bottom left,
# clear of the shear in the middle of the picture.
style="FontName=Noto Sans,FontSize=11,Bold=1,Shadow=0,Alignment=1,MarginL=16,MarginV=14"
style+=",BorderStyle=4,BackColour=&H70000000,OutlineColour=&H70000000,Outline=5"
"$here/../../docs/video/encode.sh" "$raw" "$out" \
    "scale=1280:-2:flags=lanczos,subtitles=${raw%.mp4}.srt:force_style='$style'"
