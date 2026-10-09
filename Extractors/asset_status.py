"""Check converted outputs without importing the conversion dependencies."""
import json
from pathlib import Path
import struct

from weapon_specs import WEAPONS


def read_json(path):
    try:
        return json.loads(path.read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return None


def valid_glb(path):
    """Reject missing and truncated models without loading large meshes into memory."""
    try:
        with path.open('rb') as model:
            magic, version, length = struct.unpack('<4sII', model.read(12))
        return magic == b'glTF' and version == 2 and length >= 20 and length == path.stat().st_size
    except (OSError, struct.error):
        return False


def valid_name(name):
    return isinstance(name, str) and bool(name) and name not in ('.', '..') and not any(c in name for c in '/\\:')


def weapon_errors(assets):
    errors = []
    if not valid_glb(assets / 'mason.glb'):
        errors.append('mason.glb is missing or invalid')
    catalog = read_json(assets / 'weapons.json')
    if not isinstance(catalog, dict):
        return errors + ['weapons.json is missing or invalid']
    for key in WEAPONS:
        info = catalog.get(key)
        filename = f'weapons/{key}.glb'
        if not isinstance(info, dict) or info.get('file') != filename or not valid_glb(assets / filename):
            errors.append(f'{filename} is missing, invalid, or absent from weapons.json')
    return errors


def complete_chunks(assets):
    catalog = read_json(assets / 'chunks/catalog.json')
    complete = set()
    for entry in catalog if isinstance(catalog, list) else []:
        name = entry.get('name') if isinstance(entry, dict) else None
        if not valid_name(name):
            continue
        definition = read_json(assets / 'chunks' / f'{name}.json')
        if (isinstance(definition, dict) and definition.get('name') == name
                and isinstance(definition.get('destroyables'), list)
                and valid_glb(assets / 'chunks' / f'{name}.glb')):
            complete.add(name)
    return complete


def asset_errors(assets, expected_chunks=()):
    errors = weapon_errors(assets)
    for filename, kind in (('camera_shakes.json', dict), ('chunks/materials.json', list)):
        value = read_json(assets / filename)
        if not isinstance(value, kind) or not value:
            errors.append(f'{filename} is missing, empty, or invalid')
    catalog = read_json(assets / 'chunks/catalog.json')
    if not isinstance(catalog, list) or not catalog:
        return errors + ['chunks/catalog.json is missing, empty, or invalid']
    names = [entry.get('name') if isinstance(entry, dict) else None for entry in catalog]
    if any(not valid_name(name) for name in names) or len(set(names)) != len(names):
        return errors + ['chunks/catalog.json contains invalid or duplicate names']
    complete = complete_chunks(assets)
    for name in sorted(set(names) | set(expected_chunks)):
        if name not in complete:
            errors.append(f'chunk {name} is missing from the catalog or has missing/invalid outputs')
    return errors


if __name__ == '__main__':
    assets = Path(__file__).resolve().parents[1] / 'LocalData/Assets'
    errors = asset_errors(assets)
    if errors:
        print('Assets are incomplete:\n- ' + '\n- '.join(errors))
        raise SystemExit(1)
    print('Converted assets are ready.')
