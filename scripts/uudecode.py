"""uudecode libarchive test fixtures: uudecode.py OUT_DIR FILE.uu..."""
import binascii
import os
import sys

out_dir, files = sys.argv[1], sys.argv[2:]
for path in files:
    name = os.path.basename(path)[: -len(".uu")]
    if name.endswith(".Z"):
        continue  # compress(1)-wrapped; not an archive we read
    name = name.removeprefix("test_read_format_")
    data = bytearray()
    started = False
    with open(path, "rb") as f:
        for line in f:
            if not started:
                started = line.startswith(b"begin ")
                continue
            if line.strip() in (b"end", b"`", b""):
                if line.strip() == b"end":
                    break
                continue
            try:
                data += binascii.a2b_uu(line)
            except binascii.Error:
                nbytes = (((line[0] - 32) & 63) * 4 + 5) // 3
                data += binascii.a2b_uu(line[:nbytes])
    with open(os.path.join(out_dir, name), "wb") as f:
        f.write(data)
