"""Convert gameplay tables the sandbox reads at runtime from the user's extracted xtbl files.

Writes LocalData/Assets/camera_shakes.json: every `camera_shake.xtbl` entry with the fields that
`FUN_004b8920` loads (missing values read as 0, as the game's float parser returns).
Usage: python Extractors/build_tables.py
"""
import json
import xml.etree.ElementTree as ET
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]
MISC = PROJECT / 'LocalData/Extracted/MiscV3'
OUT = PROJECT / 'LocalData/Assets'

# Channel -> fields read by FUN_004b8920 (record offsets 0x28..0x98).
CHANNELS = {
    'Wobble': ('Pitch', 'Roll', 'Yaw', 'Variation'),
    'Destabilization': ('Pitch', 'Roll', 'Yaw', 'Frequency'),
    'Wander': ('Pitch', 'Yaw', 'Frequency'),
    'Jitter': ('Pitch', 'Roll', 'Yaw'),
    'Oscillation1': ('Pitch', 'Roll', 'Yaw', 'Frequency'),
    'Oscillation2': ('Pitch', 'Roll', 'Yaw', 'Frequency'),
    'Direct': ('Pitch', 'Roll', 'Yaw'),
    'Wander_Direct': ('Pitch', 'Yaw'),
}
GRAPH = ('Time', 'Wobble', 'Destable', 'Wander', 'Jitter', 'Oscillation1', 'Oscillation2', 'Direct',
         'Wander_Direct', 'Blur', 'Strong_Vibration', 'Weak_Vibration')


def number(node, name):
    child = node.find(name) if node is not None else None
    try:
        return float(child.text) if child is not None and child.text else 0.0
    except ValueError:
        return 0.0


def camera_shakes():
    root = ET.parse(MISC / 'camera_shake.xtbl').getroot()
    shakes = {}
    for entry in root.iter('Camera_Shake'):
        name = (entry.findtext('Name') or '').strip()
        if not name:
            continue
        shake = {channel: {f.lower(): number(entry.find(channel), f) for f in fields}
                 for channel, fields in CHANNELS.items()}
        graph = entry.find('Strength_Graph')
        shake['graph'] = [{k.lower(): number(e, k) for k in GRAPH}
                          for e in (graph.iter('Strength_Element') if graph is not None else [])]
        shakes[name.lower()] = shake
    return shakes


def main():
    assert OUT.resolve().is_relative_to(PROJECT / 'LocalData')
    OUT.mkdir(parents=True, exist_ok=True)
    shakes = camera_shakes()
    (OUT / 'camera_shakes.json').write_text(json.dumps(shakes, indent=1))
    print(f'camera_shakes.json: {len(shakes)} shakes')


if __name__ == '__main__':
    main()
