"""Pretty-print `dump` JSON lines: python3 -I scripts/summarize.py < dump.jsonl"""
import json
import sys

for line in sys.stdin:
    try:
        d = json.loads(line)
    except ValueError:
        print("RAW", line.strip()[:200])
        continue
    f = d["file"].split("/")[-1]
    if "error" in d:
        print(f"{f:55s} ERR {d['error']:12s} {d.get('message', '')[:60]} {d.get('ms')}ms")
    else:
        skips = [s["reason"] for s in d["skipped"]][:4]
        print(f"{f:55s} {d['format']:4s} n={len(d['entries']):3d} skip={skips} rec={d['recovered']} {d['ms']}ms")
