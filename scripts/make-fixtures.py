"""Generates fixtures/generated/: zip files written byte by byte (so flags, charsets, extra fields
and broken structures are exactly what we want), 7z files via the `7z` CLI, and bombs.

    python3 -I scripts/make-fixtures.py

Needs `7z` and `zip` on PATH. Output is committed; rerun only to add cases.
"""

import binascii
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "fixtures", "generated")
DOS_TIME, DOS_DATE = 0x6000, 0x5B6A  # 2025-11-10 12:00
FLAG_UTF8 = 1 << 11
FLAG_DESCRIPTOR = 1 << 3
HOST_DOS, HOST_UNIX = 0, 3
PDF = b"%PDF-1.4\n" + b"edital de licitacao " * 2000 + b"\n%%EOF\n"


def deflate(data, level=6):
    c = zlib.compressobj(level, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


class Zip:
    """Minimal zip writer: each entry is (raw_name, data, options)."""

    def __init__(self):
        self.buf = bytearray()
        self.central = bytearray()
        self.count = 0

    def add(self, name, data, *, flags=0, method=8, host=HOST_DOS, mode=None, attr=0,
            extra=b"", stored_data=None, crc=None, descriptor=False, offset=None):
        name = name if isinstance(name, bytes) else name.encode("utf-8")
        payload = stored_data if stored_data is not None else (deflate(data) if method == 8 else data)
        crc = binascii.crc32(data) if crc is None else crc
        if descriptor:
            flags |= FLAG_DESCRIPTOR
        here = len(self.buf) if offset is None else offset
        if offset is None:
            lcrc, lcsize, lusize = (0, 0, 0) if descriptor else (crc, len(payload), len(data))
            self.buf += struct.pack("<IHHHHHIIIHH", 0x04034B50, 20, flags, method, DOS_TIME,
                                    DOS_DATE, lcrc, lcsize, lusize, len(name), len(extra))
            self.buf += name + extra + payload
            if descriptor:
                self.buf += struct.pack("<IIII", 0x08074B50, crc, len(payload), len(data))
        external = attr | ((mode << 16) if mode is not None else 0)
        self.central += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, (host << 8) | 20, 20, flags,
                                    method, DOS_TIME, DOS_DATE, crc, len(payload), len(data),
                                    len(name), len(extra), 0, 0, 0, external, here)
        self.central += name + extra
        self.count += 1
        return here

    def bytes(self, *, disk=0):
        cd_offset = len(self.buf)
        eocd = struct.pack("<IHHHHIIH", 0x06054B50, disk, disk, self.count, self.count,
                           len(self.central), cd_offset, 0)
        return bytes(self.buf + self.central + eocd)


def unicode_path_extra(raw_name, utf8_name):
    body = struct.pack("<BI", 1, binascii.crc32(raw_name)) + utf8_name.encode("utf-8")
    return struct.pack("<HH", 0x7075, len(body)) + body


def write(name, data):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(data)


def zips():
    cp850 = "edição_ação/anexo único.pdf".encode("cp850")
    z = Zip()
    z.add(cp850, PDF)
    z.add("edição_ação/".encode("cp850"), b"", method=0, attr=0x10)
    z.add("Relação de Itens.txt".encode("cp850"), b"itens\n")
    write("zip_cp850_noflag.zip", z.bytes())

    z = Zip()
    z.add("licitação/termo de referência.pdf", PDF, flags=FLAG_UTF8)
    z.add("licitação/日本語.txt", b"x", flags=FLAG_UTF8)
    write("zip_utf8_flag.zip", z.bytes())

    raw = "ação.txt".encode("cp850")
    z = Zip()
    z.add(raw, b"unicode path extra", extra=unicode_path_extra(raw, "ação (unicode).txt"))
    bad = "não.txt".encode("cp850")
    z.add(bad, b"bad crc in extra", extra=unicode_path_extra(b"other", "ignored.txt"))
    write("zip_unicode_path_extra.zip", z.bytes())

    z = Zip()
    z.add("relatório final.pdf", PDF, host=HOST_UNIX, mode=0o100644)
    write("zip_utf8_unix_noflag.zip", z.bytes())

    # CP850 bytes that happen to be valid UTF-8 ("ãé" = C6 82): from Windows, must stay CP850.
    z = Zip()
    z.add("MÃE_ãé.txt".encode("cp850"), b"x")
    write("zip_cp850_valid_utf8_bytes.zip", z.bytes())

    z = Zip()
    for name in ["../evil.txt", "/etc/passwd", "C:\\Windows\\win.ini", "ok/../../escape.txt",
                 "dir\\sub\\back.txt", "./dot//double.txt", "ctrl\x01char.txt", "nul\x00byte.txt",
                 "ok.txt", "..", "./"]:
        z.add(name, b"content of " + name.encode("utf-8", "surrogateescape"), method=0)
    write("zip_traversal.zip", z.bytes())

    z = Zip()
    z.add("link-to-passwd", b"/etc/passwd", method=0, host=HOST_UNIX, mode=0o120777)
    z.add("link-rel", b"../../x", method=0, host=HOST_UNIX, mode=0o120777)
    z.add("fifo", b"", method=0, host=HOST_UNIX, mode=0o010644)
    z.add("dir-by-mode", b"", method=0, host=HOST_UNIX, mode=0o040755)
    z.add("real.txt", b"real file\n", host=HOST_UNIX, mode=0o100644)
    write("zip_symlink.zip", z.bytes())

    z = Zip()
    for i in range(3):
        z.add(f"parte{i}.pdf", PDF + bytes([i]))
    full = z.bytes()
    write("zip_truncated_cd.zip", full[: len(z.buf) + 30])  # central directory cut mid-way
    write("zip_truncated_data.zip", full[: len(z.buf) - 500])  # last entry's data cut too
    write("zip_no_cd.zip", bytes(z.buf))  # nothing after the local entries

    write("zip_empty.zip", Zip().bytes())

    z = Zip()
    z.add("empty.txt", b"", method=0)
    z.add("pasta/", b"", method=0)
    z.add("pasta/vazio.txt", b"", method=8)
    write("zip_zero_byte.zip", z.bytes())

    z = Zip()
    z.add("descriptor.pdf", PDF, descriptor=True)
    z.add("descriptor2.txt", b"streamed\n" * 100, descriptor=True)
    write("zip_data_descriptor.zip", z.bytes())

    z = Zip()
    z.add("dup.txt", b"first")
    z.add("dup.txt", b"second")
    write("zip_duplicate_names.zip", z.bytes())

    z = Zip()
    z.add("bad.txt", b"payload with wrong crc", crc=0xDEADBEEF)
    write("zip_bad_crc.zip", z.bytes())

    z = Zip()
    z.add("weird.bin", b"x", method=99 + 1000)
    write("zip_unknown_method.zip", z.bytes())

    z = Zip()
    z.add("a.txt", b"spanned")
    write("zip_spanned.zip", b"PK\x07\x08" + z.bytes())

    z = Zip()
    z.add("a.txt", b"disk 1 of a split archive")
    write("zip_multidisk.zip", z.bytes(disk=1))

    write("zip_garbage_after_sig.zip", b"PK\x03\x04" + b"\xff" * 200)


def bombs():
    # Overlapping entries (David Fifield's zbsm.zip, simplified): one 21 MiB deflate kernel, 250
    # central-directory entries pointing at it. ~5.5 GB declared, ~45 KB on disk.
    kernel_plain = b"\0" * (21 * 1024 * 1024)
    kernel = deflate(kernel_plain, 9)
    crc = binascii.crc32(kernel_plain)
    z = Zip()
    z.add(b"k0", kernel_plain, stored_data=kernel, crc=crc)
    for i in range(1, 250):
        z.add(f"k{i}".encode(), kernel_plain, stored_data=kernel, crc=crc, offset=0)
    write("bomb_overlap.zip", z.bytes())

    # One entry of 2 GiB of zeros (~2 MiB): hits the ratio check long before the size limits.
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    chunk = b"\0" * (64 * 1024 * 1024)
    parts = [c.compress(chunk) for _ in range(32)]
    parts.append(c.flush())
    payload = b"".join(parts)
    crc = 0
    for _ in range(32):
        crc = binascii.crc32(chunk, crc)
    z = Zip()
    z.buf += struct.pack("<IHHHHHIIIHH", 0x04034B50, 20, 0, 8, DOS_TIME, DOS_DATE, crc,
                         len(payload), 2 * 1024**3 & 0xFFFFFFFF, 9, 0) + b"zeros.bin" + payload
    z.central += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 20, 20, 0, 8, DOS_TIME, DOS_DATE,
                             crc, len(payload), 2 * 1024**3, 9, 0, 0, 0, 0, 0, 0) + b"zeros.bin"
    z.count = 1
    write("bomb_single_2gib.zip", z.bytes())

    # Nested, 42.zip style but small: only the first level is extracted.
    inner = Zip()
    inner.add("zeros.bin", b"\0" * (4 * 1024 * 1024))
    level = inner.bytes()
    for depth in range(3):
        z = Zip()
        for i in range(8):
            z.add(f"lvl{depth}_{i}.zip", level)
        level = z.bytes()
    write("bomb_nested.zip", level)

    # Many tiny entries: max_entries.
    z = Zip()
    for i in range(20_000):
        z.add(f"f{i}.txt", b"", method=0)
    write("bomb_many_entries.zip", z.bytes())


def sevenz(tmp):
    src = os.path.join(tmp, "src")
    os.makedirs(os.path.join(src, "licitação", "anexos"))
    os.makedirs(os.path.join(src, "vazia"))
    files = {
        "licitação/edital nº 12-2025.pdf": PDF,
        "licitação/anexos/planilha orçamentária.txt": b"item;valor\n" * 500,
        "licitação/anexos/vazio.txt": b"",
        "日本語.txt": "テスト".encode("utf-8"),
    }
    for rel, data in files.items():
        with open(os.path.join(src, rel), "wb") as f:
            f.write(data)
    for root, dirs, names in os.walk(src):
        for n in dirs + names:
            os.utime(os.path.join(root, n), (1_700_000_000, 1_700_000_000), follow_symlinks=False)

    def run(name, *args):
        target = os.path.join(OUT, name)
        if os.path.exists(target):
            os.remove(target)
        subprocess.run(["7z", "a", "-bd", "-bso0", "-bsp0", *args, target, "."], cwd=src,
                       check=True)

    # The symlink exists only for this run: without -snl 7z would follow it and store the target.
    link = os.path.join(src, "link")
    os.symlink("/nonexistent/target", link)
    run("7z_utf8_solid.7z", "-m0=lzma2", "-ms=on", "-snl")
    os.remove(link)
    run("7z_nonsolid.7z", "-m0=lzma2", "-ms=off")
    run("7z_ppmd.7z", "-m0=ppmd")
    run("7z_bzip2.7z", "-m0=bzip2")
    run("7z_deflate.7z", "-m0=deflate")
    run("7z_copy.7z", "-m0=copy")
    run("7z_encrypted.7z", "-psegredo", "-mhe=off")
    run("7z_encrypted_header.7z", "-psegredo", "-mhe=on")
    run("7z_bcj2.7z", "-m0=BCJ2", "-m1=LZMA:d25", "-m2=LZMA:d19", "-m3=LZMA:d19", "-mb0:1",
        "-mb0s1:2", "-mb0s2:3")

    # A 7z whose LZMA coder declares a 4 GiB dictionary: must be refused before allocating.
    plain = os.path.join(tmp, "dict")
    os.makedirs(plain)
    with open(os.path.join(plain, "a.txt"), "wb") as f:
        f.write(b"dictionary test\n" * 100)
    target = os.path.join(tmp, "dict.7z")
    subprocess.run(["7z", "a", "-bd", "-bso0", "-bsp0", "-m0=lzma:d=1m", "-mhc=off", target, "."],
                   cwd=plain, check=True)
    data = bytearray(open(target, "rb").read())
    off, size = struct.unpack_from("<QQ", data, 12)
    header_start = 32 + off
    header = data[header_start : header_start + size]
    # LZMA props: 5 bytes (lc/lp/pb, dict size LE) right after method id 03 01 01 and size 05.
    i = header.find(b"\x03\x01\x01\x05")
    assert i >= 0, "LZMA coder not found in plain header"
    struct.pack_into("<I", header, i + 5, 0xFFFFFFFF)
    data[header_start : header_start + size] = header
    struct.pack_into("<I", data, 28, binascii.crc32(header))
    struct.pack_into("<I", data, 8, binascii.crc32(bytes(data[12:32])))
    write("7z_dict_4gib.7z", bytes(data))

    truncated = open(os.path.join(OUT, "7z_utf8_solid.7z"), "rb").read()
    write("7z_truncated.7z", truncated[: len(truncated) // 2])


def zip_cli(tmp):
    src = os.path.join(tmp, "zipsrc")
    os.makedirs(src)
    for name, data in {"aberto.txt": b"sem senha\n", "secreto.pdf": PDF}.items():
        with open(os.path.join(src, name), "wb") as f:
            f.write(data)
    target = os.path.join(OUT, "zip_zipcrypto_partial.zip")
    if os.path.exists(target):
        os.remove(target)
    subprocess.run(["zip", "-q", "-X", target, "aberto.txt"], cwd=src, check=True)
    subprocess.run(["zip", "-q", "-X", "-P", "segredo", target, "secreto.pdf"], cwd=src,
                   check=True)
    # Nested: a zip and a 7z inside a zip come back as plain entries.
    nested = os.path.join(OUT, "zip_nested_archives.zip")
    if os.path.exists(nested):
        os.remove(nested)
    shutil.copy(os.path.join(OUT, "zip_utf8_flag.zip"), os.path.join(src, "inner.zip"))
    shutil.copy(os.path.join(OUT, "7z_copy.7z"), os.path.join(src, "inner.7z"))
    subprocess.run(["zip", "-q", "-X", nested, "inner.zip", "inner.7z"], cwd=src, check=True)


def rar5_vint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def rar5_block(header_type, flags, body, data=b""):
    content = rar5_vint(header_type) + rar5_vint(flags) + body
    size_bytes = rar5_vint(len(content))
    crc = binascii.crc32(size_bytes + content)
    return struct.pack("<I", crc) + size_bytes + content + data


def rar5_file(name, payload, *, split_before=False, split_after=False, unpacked_size=None):
    raw_name = name.encode("utf-8")
    unp = len(payload) if unpacked_size is None else unpacked_size
    flags = 0x02  # HFL_DATA
    if split_before:
        flags |= 0x08  # HFL_SPLITBEFORE
    if split_after:
        flags |= 0x10  # HFL_SPLITAFTER
    # FileFlags: FHFL_CRC32 (0x04)
    body = (
        rar5_vint(len(payload))
        + rar5_vint(0x04)
        + rar5_vint(unp)
        + rar5_vint(0o100644)
        + struct.pack("<I", binascii.crc32(payload))
        + rar5_vint(0)  # method 0 (stored), RAR 5.0
        + rar5_vint(1)  # HOST5_UNIX
        + rar5_vint(len(raw_name))
        + raw_name
    )
    return rar5_block(2, flags, body, payload)


def rars():
    sig = b"Rar!\x1a\x07\x01\x00"

    # Truncated RAR5: QuickOpen locator in main header points past EOF (sets RARX_WARNING on open),
    # two complete files precede a third file cut mid-data.
    locator_payload = rar5_vint(1) + rar5_vint(1) + rar5_vint(500_000)  # MHEXTRA_LOCATOR + QLIST
    locator_extra = rar5_vint(len(locator_payload)) + locator_payload
    main_qopen = rar5_block(1, 0x01, rar5_vint(len(locator_extra)) + rar5_vint(0) + locator_extra)
    f0 = rar5_file("completo_0.pdf", PDF)
    f1 = rar5_file("completo_1.txt", b"item;valor\n" * 50)
    f2_cut = rar5_file("cortado_2.pdf", PDF + b"tail")[:-500]
    write("rar5_truncated_data.rar", sig + main_qopen + f0 + f1 + f2_cut)

    # Non-solid RAR5 middle volume (volume 2): first entry is SplitBefore, two entries in the
    # middle are complete, last entry is SplitAfter, and EndArc has EHFL_NEXTVOLUME (0x01).
    main_vol2 = rar5_block(1, 0, rar5_vint(0x01 | 0x02) + rar5_vint(1))  # MHFL_VOLUME | MHFL_VOLNUMBER
    s_prev = rar5_file("borda_anterior.pdf", b"tail-bytes", split_before=True, unpacked_size=10_000)
    mid_a = rar5_file("meio/anexo_a.pdf", PDF)
    mid_b = rar5_file("meio/orcamento.txt", b"orcamento completo\n")
    s_next = rar5_file("borda_seguinte.pdf", b"head-bytes", split_after=True, unpacked_size=10_000)
    end_next = rar5_block(5, 0, rar5_vint(0x01))
    write("rar5_volume_part2.rar", sig + main_vol2 + s_prev + mid_a + mid_b + s_next + end_next)


def main():
    os.makedirs(OUT, exist_ok=True)
    zips()
    bombs()
    rars()
    with tempfile.TemporaryDirectory() as tmp:
        sevenz(tmp)
        zip_cli(tmp)
    print("fixtures written to", OUT, file=sys.stderr)


main()
