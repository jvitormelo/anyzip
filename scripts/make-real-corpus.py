"""Builds fixtures/real/ (git-ignored) from real edital PDFs, the way portals' users make them:
Windows Explorer (CP850 names, no UTF-8 flag), Info-ZIP, 7-Zip zip, 7-Zip 7z (solid LZMA2,
fast, non-solid) and Python's zipfile. Also copies any real archives given with --copy.

    python3 -I scripts/make-real-corpus.py --pdfs ./sample-pdfs \\
        [--pdfs ./more-pdfs] [--copy DIR_WITH_REAL_ARCHIVES]
"""

import argparse
import binascii
import os
import shutil
import struct
import subprocess
import tempfile
import zipfile
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "fixtures", "real")
NAMES = [
    "Edital nº {n} - Pregão Eletrônico.pdf",
    "Anexo I - Termo de Referência.pdf",
    "Anexo II - Planilha Orçamentária.pdf",
    "Anexo III - Declarações/Modelo de Proposta Comercial.pdf",
    "Projeto Básico/Memorial Descritivo e Especificações Técnicas.pdf",
    "Minuta do Contrato - Licitação {n}.pdf",
    "Cronograma Físico-Financeiro.pdf",
    "Composição de Custos Unitários (BDI).pdf",
]


def pdfs_in(dirs):
    found = []
    for d in dirs:
        for root, _dirs, names in os.walk(d):
            found += [os.path.join(root, n) for n in names if n.lower().endswith(".pdf")]
    return sorted(found)


def windows_zip(path, members):
    """What Windows Explorer (pt-BR) writes: CP850 names, no UTF-8 flag, deflate, host DOS."""
    buf, central = bytearray(), bytearray()
    for name, data in members:
        raw = name.encode("cp850")
        comp = zlib.compressobj(6, zlib.DEFLATED, -15)
        payload = comp.compress(data) + comp.flush()
        crc = binascii.crc32(data)
        offset = len(buf)
        buf += struct.pack("<IHHHHHIIIHH", 0x04034B50, 20, 0, 8, 0x6000, 0x5B6A, crc,
                           len(payload), len(data), len(raw), 0) + raw + payload
        central += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 20, 20, 0, 8, 0x6000, 0x5B6A,
                               crc, len(payload), len(data), len(raw), 0, 0, 0, 0, 0x20,
                               offset) + raw
    eocd = struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, len(members), len(members), len(central),
                       len(buf), 0)
    with open(path, "wb") as f:
        f.write(buf + central + eocd)


def stage(tmp, members):
    src = os.path.join(tmp, "src")
    shutil.rmtree(src, ignore_errors=True)
    for name, data in members:
        full = os.path.join(src, name)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "wb") as f:
            f.write(data)
    return src


def build(tag, members, tmp):
    def out(name):
        target = os.path.join(OUT, f"{tag}_{name}")
        if os.path.exists(target):
            os.remove(target)
        return target

    windows_zip(out("windows.zip"), members)
    with zipfile.ZipFile(out("python.zip"), "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in members:
            z.writestr(name, data)
    src = stage(tmp, members)
    quiet = ["-bd", "-bso0", "-bsp0"]
    subprocess.run(["zip", "-q", "-r", "-X", out("infozip.zip"), "."], cwd=src, check=True)
    subprocess.run(["7z", "a", *quiet, "-tzip", out("7zip.zip"), "."], cwd=src, check=True)
    subprocess.run(["7z", "a", *quiet, out("solid.7z"), "."], cwd=src, check=True)
    subprocess.run(["7z", "a", *quiet, "-mx=1", out("fast.7z"), "."], cwd=src, check=True)
    subprocess.run(["7z", "a", *quiet, "-ms=off", out("nonsolid.7z"), "."], cwd=src, check=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pdfs", action="append", default=[])
    ap.add_argument("--copy", action="append", default=[])
    args = ap.parse_args()
    os.makedirs(OUT, exist_ok=True)

    for d in args.copy:
        for name in sorted(os.listdir(d)):
            if name.lower().endswith((".zip", ".7z", ".rar")):
                shutil.copy(os.path.join(d, name), os.path.join(OUT, f"copied_{name}"))

    pdfs = pdfs_in(args.pdfs)
    by_size = sorted(pdfs, key=os.path.getsize)
    small = by_size[: len(by_size) // 3]
    medium = by_size[len(by_size) // 3 : 2 * len(by_size) // 3]
    large = by_size[2 * len(by_size) // 3 :]
    plan = {
        "s2": small[:2],
        "s8": small[10:18],
        "m3": medium[:3],
        "m8": medium[20:28],
        "l4": large[:4],
        "l8": large[-8:],
    }
    with tempfile.TemporaryDirectory() as tmp:
        for tag, files in plan.items():
            members = []
            for i, path in enumerate(files):
                with open(path, "rb") as f:
                    members.append((NAMES[i % len(NAMES)].format(n=f"{i + 1}-2026"), f.read()))
            build(tag, members, tmp)
    total = sum(os.path.getsize(os.path.join(OUT, n)) for n in os.listdir(OUT))
    print(f"{len(os.listdir(OUT))} archives, {total / 1e6:.0f} MB in {OUT}")


main()
