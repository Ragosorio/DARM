#!/bin/sh
# Derives the tiny media files used by examples/snippets/03-media.darml from the
# original Lighthouse Protocol media (run tools/make_lighthouse_media.py first).
# Output: snippets/assets/{beam.mp4,beam.jpg,beam.en.vtt,bell.m4a}. The hand-written
# snippets/assets/tide.svg and moon.svg are source files, not generated.
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
src="$here/../the-lighthouse-protocol/media"
out="$here/../snippets/assets"
mkdir -p "$out"

# 6 s, 480x270 excerpt that contains both flashes (t = 3.0 s and 5.5 s of the source).
ffmpeg -hide_banner -loglevel error -y -ss 1.5 -t 6 -i "$src/beam.mp4" \
    -vf "scale=480:270:flags=lanczos" -c:v libx264 -profile:v high -pix_fmt yuv420p -crf 30 -preset slow \
    -c:a aac -b:a 64k -ac 2 -movflags +faststart -map_metadata -1 -fflags +bitexact -flags:v +bitexact -flags:a +bitexact \
    "$out/beam.mp4"
ffmpeg -hide_banner -loglevel error -y -ss 1.5 -i "$out/beam.mp4" -frames:v 1 -q:v 4 -map_metadata -1 -fflags +bitexact "$out/beam.jpg"

# First stroke of the fog bell, 8 s, mono.
ffmpeg -hide_banner -loglevel error -y -t 8 -i "$src/bell.m4a" -af "afade=t=out:st=6.5:d=1.5" \
    -c:a aac -b:a 56k -ac 1 -movflags +faststart -map_metadata -1 -fflags +bitexact -flags:a +bitexact "$out/bell.m4a"

cat > "$out/beam.en.vtt" <<'VTT'
WEBVTT

1
00:00:00.000 --> 00:00:01.900
[Swell washing over rock]

2
00:00:01.900 --> 00:00:03.700
[Water draining from the ledges]

3
00:00:03.700 --> 00:00:06.000
[A second, smaller swell]
VTT
echo "snippet assets written to $out"
