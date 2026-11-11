"""Differential test: anyzip vs bsdtar.

    python3 -I scripts/diff-bsdtar.py [--dump PATH] [--known fixtures/diffs.json] FILE...

For each archive, compares the set of (path, md5) of regular files. bsdtar runs with
`--options zip:hdrcharset=CP850` into an empty directory; symlinks it creates are ignored, matching
our skipped entries. When anyzip returns an error, the comparison is against "no files".

Exit 1 if any archive differs and is not listed in the known-diffs file with a reason.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def bsdtar_files(archive):
    with tempfile.TemporaryDirectory(prefix="anyzip-diff-") as out:
        # `--passphrase ""` avoids libarchive's 1s-per-entry anti-bruteforce sleep on encrypted zips.
        proc = subprocess.run(
            ["bsdtar", "-xf", os.path.abspath(archive), "-C", out, "--no-same-owner",
             "--no-same-permissions", "--passphrase", "-", "--options", "zip:hdrcharset=CP850"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            timeout=120, start_new_session=True)
        for root, dirs, _names in os.walk(out):
            for d in dirs:
                os.chmod(os.path.join(root, d), 0o700)
        files = {}
        for root, _dirs, names in os.walk(out):
            for name in names:
                full = os.path.join(root, name)
                if os.path.islink(full) or not os.path.isfile(full):
                    continue
                rel = os.path.relpath(full, out).replace(os.sep, "/")
                os.chmod(full, 0o600)  # archives may carry mode 000
                with open(full, "rb") as f:
                    files[rel] = hashlib.file_digest(f, "md5").hexdigest()
        return files, proc.returncode, proc.stderr.decode("utf-8", "replace").strip()


def anyzip_results(dump, archives):
    proc = subprocess.run([dump, *archives], capture_output=True, check=True, timeout=600)
    return {r["file"]: r for r in map(json.loads, proc.stdout.decode().splitlines())}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dump", default=os.path.join(ROOT, "target/release/examples/dump"))
    ap.add_argument("--known", default=os.path.join(ROOT, "fixtures/diffs.json"))
    ap.add_argument("--verbose", "-v", action="store_true")
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()

    known = {}
    if os.path.exists(args.known):
        with open(args.known) as f:
            known = json.load(f)

    ours = anyzip_results(args.dump, args.files)
    same = expected = new = 0
    for archive in args.files:
        name = os.path.basename(archive)
        mine = ours[archive]
        # Known bomb / huge-dictionary fixtures would make bsdtar allocate 4 GiB or write GiBs on
        # small CI runners; anyzip already rejected them with `tooLarge` as expected.
        if mine.get("error") == "tooLarge" and name in known:
            expected += 1
            if args.verbose:
                print(f"expected  {name}: {known[name]}")
            continue
        theirs, code, err = bsdtar_files(archive)
        mine_files = {e["path"]: e["md5"] for e in mine.get("entries", [])}
        if mine_files == theirs and ("error" not in mine or not theirs):
            same += 1
            if args.verbose:
                print(f"same      {name}")
            continue
        only_mine = sorted(set(mine_files.items()) - set(theirs.items()))
        only_theirs = sorted(set(theirs.items()) - set(mine_files.items()))
        detail = []
        if "error" in mine:
            detail.append(f"anyzip error {mine['error']}: {mine.get('message', '')[:80]}")
        if code != 0:
            detail.append(f"bsdtar exit {code}: {err[:120]}")
        detail += [f"  only anyzip: {p} {m[:8]}" for p, m in only_mine[:6]]
        detail += [f"  only bsdtar: {p} {m[:8]}" for p, m in only_theirs[:6]]
        if name in known:
            expected += 1
            if args.verbose:
                print(f"expected  {name}: {known[name]}")
        else:
            new += 1
            print(f"DIFF      {name}")
            for line in detail:
                print(f"          {line}")
    print(f"\n{same} same, {expected} expected differences, {new} new differences")
    return 1 if new else 0


sys.exit(main())
