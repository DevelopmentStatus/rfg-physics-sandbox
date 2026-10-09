"""Read the original PC v3 archive layouts verified by this project.

Supports uncompressed VPP and compressed/condensed STR2 only. Directory
offsets are authoritative; do not add padding to packed STR2 entries.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib

PROJECT = Path(__file__).resolve().parents[1]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def decode(blob):
    def u32(offset):
        return struct.unpack_from('<I', blob, offset)[0]
    if len(blob) < 2048 or (u32(0), u32(4)) != (0x51890ACE, 3):
        raise ValueError('Expected little-endian original PC archive version 3')
    flags, count, file_size, directory_size, names_size, raw_size, packed_size = (
        u32(i) for i in range(0x14c, 0x16c, 4) if i != 0x150)
    if file_size != len(blob) or directory_size != count * 28:
        raise ValueError('Archive length or directory size mismatch')
    align = lambda n: (n + 2047) & ~2047
    names_offset = 2048 + align(directory_size)
    data_offset = names_offset + align(names_size)
    names = blob[names_offset:names_offset + names_size]
    eof = None
    if flags == 0:
        payload = blob[data_offset:]
    elif flags == 3:
        if data_offset + packed_size != len(blob):
            raise ValueError('Compressed payload length mismatch')
        inflater = zlib.decompressobj()
        payload = inflater.decompress(blob[data_offset:], raw_size + 1)
        eof = inflater.eof
        if inflater.unused_data or inflater.unconsumed_tail:
            raise ValueError('Unexpected compressed trailing data')
        # Original fixture has a size-delimited stream without a zlib end marker.
        # Exact output size and complete directory coverage remain mandatory.
    elif flags == 1:
        # Per-entry compression (mp_common, terr01_precache): one zlib stream per entry,
        # stored in directory order at 2048-byte aligned file positions. Directory offsets
        # address the 2048-aligned decompressed layout, so rebuild that layout.
        payload = bytearray(raw_size)
        cursor = data_offset
        for i in range(count):
            _, _, offset, _, size, compressed, _ = struct.unpack_from('<7I', blob, 2048 + i * 28)
            inflater = zlib.decompressobj()  # streams end without a zlib end marker, as in STR2
            value = inflater.decompress(blob[cursor:cursor + compressed], size + 1)
            if len(value) != size or offset + size > raw_size:
                raise ValueError('Per-entry decompressed size mismatch')
            payload[offset:offset + size] = value
            cursor = align(cursor + compressed)
        if cursor != align(data_offset + packed_size) or align(data_offset + packed_size) < len(blob):
            raise ValueError('Per-entry compressed streams do not cover the payload')
        payload = bytes(payload)
    else:
        raise ValueError(f'Unverified flags {flags}')
    if len(payload) != raw_size:
        raise ValueError('Decompressed payload length mismatch')
    entries, seen, cursor = [], set(), 0
    for i in range(count):
        fields = struct.unpack_from('<7I', blob, 2048 + i * 28)
        name_offset, unknown, offset, name_hash, size, compressed, unknown2 = fields
        if name_offset >= len(names) or b'\0' not in names[name_offset:]:
            raise ValueError('Invalid name offset')
        name = names[name_offset:].split(b'\0', 1)[0].decode('ascii')
        if not name or Path(name).name != name or any(c in name for c in '/\\:'):
            raise ValueError('Unsafe member path')
        if name.casefold() in seen or offset < cursor or offset + size > len(payload):
            raise ValueError('Duplicate or overlapping/out-of-range entry')
        if flags == 3 and offset != cursor:
            # Condensed archives such as anims.vpp_pc pad entries to 16 bytes.
            # Accept only that exact alignment, and only if the padding is zero.
            aligned = (cursor + 15) & ~15
            if offset != aligned or any(payload[cursor:offset]):
                raise ValueError('Unverified gap in condensed payload')
        seen.add(name.casefold())
        value = payload[offset:offset + size]
        entries.append((name, value, dict(offset=offset, size=size,
                       sha256=digest(value), raw_directory=list(fields))))
        cursor = offset + size
    if cursor != len(payload):
        raise ValueError('Directory does not cover payload end')
    return entries, dict(flags=flags, count=count, data_offset=data_offset,
                         payload_size=len(payload), zlib_end_marker=eof)


def extract(source, output):
    output = output.resolve()
    if not output.is_relative_to(PROJECT / 'LocalData'):
        raise ValueError('Outputs must resolve inside project LocalData')
    blob = source.read_bytes()
    entries, info = decode(blob)
    if output.exists():
        raise ValueError('Use a fresh output directory')
    output.mkdir(parents=True)
    for name, value, metadata in entries:
        (output / name).write_bytes(value)
    manifest = dict(schema_version=1, source=source.name,
                    source_sha256=digest(blob), **info,
                    entries=[dict(name=n, **m) for n, _, m in entries])
    (output / 'extraction_manifest.json').write_text(json.dumps(manifest, indent=2))
    print(json.dumps(dict(output=output.name, **info)))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    extract(args.source, args.output)
