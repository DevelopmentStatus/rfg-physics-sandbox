"""Regenerate every asset the Bevy sandbox loads from the user's own Red Faction Guerrilla install.

Usage: python Extractors/extract_all.py --game <RFG install dir> [--force] [--only STEP[,STEP]]
                                       [--chunks NAME ...] [--clean]

Steps (run in this order, each skipped when its output already exists unless --force):
  misc    build/pc/cache/misc.vpp_pc    -> LocalData/Extracted/MiscV3
  items   build/pc/cache/items.vpp_pc   -> LocalData/Extracted/Items
  anims   build/pc/cache/anims.vpp_pc   -> LocalData/Extracted/AnimsV3
  player  build/pc/cache/humans.vpp_pc  -> LocalData/Extracted/Player/rfg_PC
  chunks  every *.vpp_pc                -> LocalData/Extracted/Chunks (680 chunks, 663 texture files)
  assets  build_assets.py               -> LocalData/Assets/mason.glb, weapons/*.glb, weapons.json
  tables  build_tables.py               -> LocalData/Assets/camera_shakes.json
  build   build_chunks.py               -> LocalData/Assets/chunks (about 40 minutes for all chunks)

--chunks limits the `build` step to the named chunks (a quick test, e.g. 0203house01_a).
--clean deletes LocalData/Extracted afterwards (about 1.4 GB) once everything is built.
Needs numpy, Pillow and scipy (ExtractAssets.bat installs them).
"""
import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]
LOCAL = PROJECT / 'LocalData'
EXTRACTED = LOCAL / 'Extracted'
ASSETS = LOCAL / 'Assets'
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import archive_v3  # noqa: E402
import extract_player  # noqa: E402
from asset_status import asset_errors, complete_chunks, read_json, weapon_errors  # noqa: E402

EXPECTED_EXE_SHA256 = '0606bbed95481ee6d54b5a926adaccaa29db6e2e42e2fff09fe9dfd25946c72d'
STEPS = ('misc', 'items', 'anims', 'player', 'chunks', 'assets', 'tables', 'build')
BATCH = 40


def exe_hash(game):
    digest = hashlib.sha256()
    with open(game / 'RFG.exe', 'rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def run(script, *args, env=None):
    command = [sys.executable, str(HERE / script), *args]
    if subprocess.run(command, cwd=PROJECT, env=env).returncode:
        sys.exit(f'{script} failed')


def extract_archive(source, out):
    """Extract a flat archive into `out`, atomically: a half-written folder is never left behind."""
    partial = out.with_name(out.name + '.partial')
    for path in (partial, out):
        if path.exists():
            shutil.rmtree(path)
    out.parent.mkdir(parents=True, exist_ok=True)
    archive_v3.extract(source, partial)
    partial.rename(out)


def chunk_names():
    import json
    index = json.loads((EXTRACTED / 'Chunks/index.json').read_text())
    return sorted(n[:-8] for n in index['chunks'] if n.endswith('.cchk_pc'))


def built_chunks():
    return complete_chunks(ASSETS)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--game', type=Path, default=os.environ.get('RFG_GAME_DIR'))
    parser.add_argument('--force', action='store_true')
    parser.add_argument('--only', default='')
    parser.add_argument('--chunks', nargs='*')
    parser.add_argument('--clean', action='store_true')
    args = parser.parse_args()
    if args.clean and (args.only or args.chunks is not None):
        parser.error('--clean requires a full extraction; omit --only and --chunks')
    if args.chunks == []:
        parser.error('--chunks requires at least one chunk name')
    if not args.game or not (args.game / 'RFG.exe').is_file():
        sys.exit('Game folder with RFG.exe not found. Pass --game "<Red Faction Guerrilla folder>".')
    cache = args.game / 'build/pc/cache'
    if not cache.is_dir():
        sys.exit(f'{cache} not found. This does not look like a Red Faction Guerrilla install.')
    if exe_hash(args.game) != EXPECTED_EXE_SHA256 and os.environ.get('RFG_ALLOW_OTHER_VERSION') != '1':
        sys.exit('RFG.exe is not the supported build (1.0.2.1, SHA256 0606BBED...946C72D).\n'
                 'Other builds (for example Re-Mars-tered) use different chunk formats. '
                 'Set RFG_ALLOW_OTHER_VERSION=1 to try anyway.')
    only = {s for s in args.only.split(',') if s}
    unknown = only - set(STEPS)
    if unknown:
        sys.exit(f'Unknown step(s): {", ".join(sorted(unknown))}. Steps: {", ".join(STEPS)}')
    env = dict(os.environ, RFG_GAME_DIR=str(args.game), PYTHONIOENCODING='utf-8')
    started = time.time()

    def wanted(step, done):
        if only and step not in only:
            return False
        if done and not args.force:
            print(f'[{step}] already done (use --force to redo)')
            return False
        print(f'[{step}] running...', flush=True)
        return True

    for step, vpp in (('misc', 'misc'), ('items', 'items'), ('anims', 'anims')):
        out = EXTRACTED / {'misc': 'MiscV3', 'items': 'Items', 'anims': 'AnimsV3'}[step]
        if wanted(step, (out / 'extraction_manifest.json').exists()):
            extract_archive(cache / f'{vpp}.vpp_pc', out)
    player = EXTRACTED / 'Player/rfg_PC'
    if wanted('player', (player / 'extraction_manifest.json').exists()):
        extract_player.extract(cache / 'humans.vpp_pc', player)
    if wanted('chunks', (EXTRACTED / 'Chunks/index.json').exists()):
        run('extract_chunks.py', str(cache), env=env)
    if wanted('assets', not weapon_errors(ASSETS)):
        run('build_assets.py', env=env)
    tables = read_json(ASSETS / 'camera_shakes.json')
    if wanted('tables', isinstance(tables, dict) and bool(tables)):
        run('build_tables.py', env=env)

    names = []
    if not only or 'build' in only:
        available = chunk_names()
        names = args.chunks if args.chunks else available
        unknown_chunks = set(names) - set(available)
        if unknown_chunks:
            sys.exit('Unknown chunk(s): ' + ', '.join(sorted(unknown_chunks)))
        if not names:
            sys.exit('No chunks found in the extracted index.')
        todo = names if args.force else [n for n in names if n not in built_chunks()]
        if not todo:
            print('[build] already done (use --force to redo)')
        for i in range(0, len(todo), BATCH):  # batches keep catalog.json current, so an interrupted run resumes
            batch = todo[i:i + BATCH]
            print(f'[build] chunks {i + 1}-{i + len(batch)} of {len(todo)}', flush=True)
            run('build_chunks.py', *batch, env=env)

    chunks_built = len(built_chunks())
    print(f'\nFinished in {(time.time() - started) / 60:.1f} min. Chunks built: {chunks_built}.')
    if only:
        print('Selected steps completed. Run a full extraction before playing.')
        return
    errors = asset_errors(ASSETS, names)
    if errors:
        print('Assets are incomplete:\n- ' + '\n- '.join(errors))
        sys.exit(1)
    if args.clean:
        shutil.rmtree(EXTRACTED, ignore_errors=True)
        print('Removed LocalData/Extracted')
    print('All requested assets verified. Run Play.bat.')


if __name__ == '__main__':
    main()
