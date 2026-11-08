#!/usr/bin/env python3
"""Build the lamp's microSD card contents under audio/sd/mp3.

The card is numbered by what the file is for, and the lamp never counts files:

    0001-0100   adhan          (the 21 downloaded ones are 0001-0021)
    0101-0200   alarm sounds   (the ringtones, in the order of tools/ringtone_order.txt)

Every slot nobody has filled gets a placeholder recording, a robot voice that says which slot it is
and to replace the file, so choosing an empty slot is never silence.

    python3 tools/audio_card.py ringtones      convert audio/ringtones-source -> 0102 and up
    python3 tools/audio_card.py placeholders   a spoken placeholder for every empty slot (needs espeak-ng)
    python3 tools/audio_card.py index          write audio/sd/INDEX.md and docs/audio-credits.md

Needs ffmpeg and ffprobe. The placeholders need `espeak-ng` (on Arch: sudo pacman -S espeak-ng).
audio/ is git-ignored on purpose, so this script and docs/audio-credits.md are what is kept.
"""
import json
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AUDIO = ROOT / "audio"
SRC = AUDIO / "ringtones-source"
OUT = AUDIO / "sd" / "mp3"
CREDITS = [SRC / "credits.json", SRC / "credits_ov.json"]   # Wikimedia Commons ones, and the Openverse ones
ORDER = ROOT / "tools" / "ringtone_order.txt"
FIRST_RINGTONE = 101
ADHAN_SLOTS = range(1, 101)
ALARM_SLOTS = range(101, 201)
CLIP_S = 25                                # longest a ringtone is kept
SHORT_S = 6.0                              # shorter than this, a clip is repeated with gaps up to about:
FILL_S = 10.0
SAMPLE_RATE = "22050"


def run(*cmd, **kw):
    return subprocess.run(cmd, check=True, capture_output=True, text=True, **kw)


def duration(path):
    out = run("ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path))
    try:
        return float(out.stdout.strip())
    except ValueError:
        return 0.0


def to_mp3(src, dst, fade=True):
    """Mono, 22.05 kHz, leading silence off, level matched, at most CLIP_S seconds with a fade out.
    A clip shorter than SHORT_S is repeated with gaps up to about FILL_S: an air horn alone would
    repeat every two seconds once the lamp plays it again."""
    length = min(duration(src), CLIP_S)
    filters = ["silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.05"]
    if length < SHORT_S:
        gap = 1.2
        reps = max(2, int(FILL_S // (length + gap) + 0.5))
        filters.append(f"apad=pad_dur={gap}")
        filters.append(f"aloop=loop={reps - 1}:size=2000000")
        length = reps * (length + gap)
    filters.append("loudnorm=I=-16:TP=-1.5:LRA=11")
    if fade and length > 6:
        filters.append(f"afade=t=out:st={length - 3:.1f}:d=3")
    run("ffmpeg", "-v", "error", "-y", "-i", str(src), "-vn", "-t", str(CLIP_S),
        "-af", ",".join(filters), "-ac", "1", "-ar", SAMPLE_RATE, "-b:a", "64k",
        "-map_metadata", "-1", "-id3v2_version", "0", "-write_xing", "0", str(dst))


def load_credits():
    """Every ringtone that has a file, in the order of tools/ringtone_order.txt (then the rest)."""
    entries = {}
    for path in CREDITS:
        if path.exists():
            for e in json.loads(path.read_text()):
                entries[e["slug"]] = e
    order = [l.strip() for l in ORDER.read_text().splitlines() if l.strip() and not l.startswith("#")]
    ranked = [entries[sl] for sl in order if sl in entries]
    rest = [e for sl, e in entries.items() if sl not in order]
    for e in rest:
        print(f"note: {e['slug']} is not in tools/ringtone_order.txt, so it goes last")
    return ranked + rest


def ringtones():
    credits = load_credits()
    OUT.mkdir(parents=True, exist_ok=True)
    slot = FIRST_RINGTONE
    done = []
    for entry in credits:
        name = entry.get("file")
        src = SRC / name if name else None
        if not src or not src.exists():
            print(f"skip {entry['slug']}: not downloaded")
            continue
        dst = OUT / f"{slot:04d}.mp3"
        to_mp3(src, dst)
        entry["slot"] = slot
        done.append(entry)
        print(f"{slot:04d}  {entry['slug']:20} {duration(dst):5.1f} s  {entry.get('license', '?')}")
        slot += 1
        if slot > 200:
            raise SystemExit("more ringtones than alarm slots")
    (SRC / "slots.json").write_text(json.dumps(done, ensure_ascii=False, indent=1))
    print(f"{len(done)} ringtones, slots {FIRST_RINGTONE:04d}-{slot - 1:04d}")


def spoken(n):
    return " ".join("zero one two three four five six seven eight nine".split()[int(c)] for c in f"{n:04d}")


def placeholders():
    if not shutil.which("espeak-ng"):
        raise SystemExit("espeak-ng is not installed: sudo pacman -S espeak-ng")
    OUT.mkdir(parents=True, exist_ok=True)
    made = 0
    tmp = AUDIO / "tmp-placeholder.wav"
    for n in list(ADHAN_SLOTS) + list(ALARM_SLOTS):
        dst = OUT / f"{n:04d}.mp3"
        if dst.exists():
            continue
        if n in ADHAN_SLOTS:
            text = (f"Adhan slot {n} is empty. To use it, replace this file with an audio named "
                    f"{spoken(n)}, dot m p 3.")
        else:
            text = (f"Alarm sound slot {n - 100} is empty. To use it, replace this file with an audio "
                    f"named {spoken(n)}, dot m p 3.")
        run("espeak-ng", "-v", "en-us", "-s", "145", "-w", str(tmp), text)
        run("ffmpeg", "-v", "error", "-y", "-i", str(tmp), "-af", "loudnorm=I=-18:TP=-2:LRA=7",
            "-ac", "1", "-ar", SAMPLE_RATE, "-b:a", "48k", "-map_metadata", "-1",
            "-id3v2_version", "0", "-write_xing", "0", str(dst))
        made += 1
    tmp.unlink(missing_ok=True)
    print(f"{made} placeholders written")


def index():
    slots = SRC / "slots.json"
    credits = json.loads(slots.read_text()) if slots.exists() else []
    lines = ["# Lamp sounds", "",
             "Adhan 0001-0100, alarm sounds 0101-0200. A slot with no real sound holds a spoken placeholder.", "",
             "| File | Sound | Author | Licence | Source |", "|---|---|---|---|---|"]
    lines.append("| 0001-0021 | Adhan, 21 recordings | IslamCan | see its copyright page | "
                 "https://www.islamcan.com/audio/adhan/index.shtml |")
    for e in credits:
        if "slot" not in e:
            continue
        who = (e.get("artist") or "unknown").replace("|", "/")
        lines.append(f"| {e['slot']:04d} | {e['kind']} | {who} | {e.get('license', '?')} | {e.get('page', '')} |")
    text = "\n".join(lines) + "\n"
    (AUDIO / "sd" / "INDEX.md").write_text(text)
    (ROOT / "docs" / "audio-credits.md").write_text(
        text + "\nThe ringtones come from Wikimedia Commons, and, through Openverse, from Freesound and "
        "Jamendo. Each is trimmed to 25 s (a very short one is repeated with gaps), level matched and "
        "converted to mono MP3, which counts as an adaptation: the licence of each applies to its copy. "
        "CC BY and CC BY-SA need the credit above; CC BY-SA also asks that the adapted copy carry the same "
        "licence. Ask IslamCan before selling a lamp with its adhan on the card.\n")
    print("index written")


if __name__ == "__main__":
    steps = {"ringtones": ringtones, "placeholders": placeholders, "index": index}
    if len(sys.argv) != 2 or sys.argv[1] not in steps:
        raise SystemExit(__doc__)
    steps[sys.argv[1]]()
