"""Extract every destructible chunk (cchk_pc/gchk_pc) and every texture bank from the user's install.

Chunks (buildings, walls, props) live in mp_common/terr01_precache (flags 1 archives), inside the
level str2 files of terr01_l0/l1, the MP and Wrecking Crew maps, and in chunks.vpp_pc. The first
copy of each name is kept; later copies are compared and differences reported.
Textures: single-texture *.cvbm_pc/gvbm_pc and multi-texture *.cpeg_pc/gpeg_pc from the same sources.

Writes LocalData/Extracted/Chunks/{chunks,textures}/ and an index.json with each file's source.
Usage: python Extractors/extract_chunks.py [game cache dir]
The cache dir defaults to <RFG_GAME_DIR>/build/pc/cache (ExtractAssets.bat sets RFG_GAME_DIR).
"""
import hashlib
import json
import os
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import archive_v3  # noqa: E402

PROJECT = Path(__file__).resolve().parents[1]
OUT = PROJECT / 'LocalData/Extracted/Chunks'
DEFAULT_CACHE = Path(os.environ.get('RFG_GAME_DIR', '')) / 'build/pc/cache'
CHUNK_EXT = ('.cchk_pc', '.gchk_pc')
TEXTURE_EXT = ('.cvbm_pc', '.gvbm_pc', '.cpeg_pc', '.gpeg_pc')


def vpp_entries(path):
    """Yield (name, bytes) for an archive, reading flag-0 entries lazily (terr01 is ~900 MB)."""
    with open(path, 'rb') as f:
        head = f.read(2048)
        u32 = lambda o: struct.unpack_from('<I', head, o)[0]
        if (u32(0), u32(4)) != (0x51890ACE, 3):
            return
        flags = u32(0x14c)
        if flags != 0:
            entries, _ = archive_v3.decode(path.read_bytes())
            yield from ((n, v) for n, v, _ in entries)
            return
        count, directory, names_size = u32(0x154), u32(0x15c), u32(0x160)
        align = lambda n: (n + 2047) & ~2047
        table = f.read(directory)
        f.seek(2048 + align(directory))
        names = f.read(names_size)
        data = 2048 + align(directory) + align(names_size)
        for i in range(count):
            name_offset, _, offset, _, size, _, _ = struct.unpack_from('<7I', table, i * 28)
            name = names[name_offset:].split(b'\0', 1)[0].decode('ascii')
            f.seek(data + offset)
            yield name, f.read(size)


def main():
    cache = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_CACHE
    if not cache.is_dir():
        sys.exit(f'Game cache folder not found: {cache}. Set RFG_GAME_DIR or pass the build/pc/cache folder.')
    assert OUT.resolve().is_relative_to(PROJECT / 'LocalData')
    for sub in ('chunks', 'textures'):
        (OUT / sub).mkdir(parents=True, exist_ok=True)
    index = dict(chunks={}, textures={}, conflicts=[])
    seen = {}

    def keep(name, value, source):
        kind = 'chunks' if name.endswith(CHUNK_EXT) else 'textures'
        key = name.lower()
        digest = hashlib.sha256(value).hexdigest()
        if key in seen:
            if seen[key] != digest:
                index['conflicts'].append(dict(name=name, source=source))
            return
        seen[key] = digest
        (OUT / kind / key).write_bytes(value)
        index[kind][key] = dict(source=source, size=len(value), sha256=digest)

    for archive in sorted(cache.glob('*.vpp_pc')):
        if archive.name.startswith(('sounds', 'voices', 'soundheaders', 'anims', 'table')):
            continue
        hits = 0
        for name, value in vpp_entries(archive):
            if name.endswith(CHUNK_EXT + TEXTURE_EXT):
                keep(name, value, archive.name)
                hits += 1
            elif name.endswith('.str2_pc'):
                try:
                    members, _ = archive_v3.decode(value)
                except ValueError as error:
                    print(f'{archive.name}/{name}: {error}')
                    continue
                for member, data, _ in members:
                    if member.endswith(CHUNK_EXT + TEXTURE_EXT):
                        keep(member, data, f'{archive.name}/{name}')
                        hits += 1
        print(f'{archive.name}: {hits} chunk/texture files')
    (OUT / 'index.json').write_text(json.dumps(index, indent=1))
    print(len(index['chunks']) // 2, 'chunks,', len(index['textures']), 'texture files,',
          len(index['conflicts']), 'name conflicts')


if __name__ == '__main__':
    main()
