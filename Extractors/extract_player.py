"""Extract Mason's character archive: humans.vpp_pc -> rfg_PC.str2_pc -> LocalData/Extracted/Player/rfg_PC.

Only rfg_PC.str2_pc is used from humans.vpp_pc (body, coat and textures; build_assets.py reads them).
Usage: python Extractors/extract_player.py <humans.vpp_pc> [output dir]
"""
import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import archive_v3  # noqa: E402

PROJECT = Path(__file__).resolve().parents[1]
MEMBER = 'rfg_PC.str2_pc'


def extract(humans, output):
    output = output.resolve()
    if not output.is_relative_to(PROJECT / 'LocalData'):
        raise ValueError('Outputs must resolve inside project LocalData')
    entries, _ = archive_v3.decode(humans.read_bytes())
    members = {name: value for name, value, _ in entries}
    if MEMBER not in members:
        raise ValueError(f'{MEMBER} not found in {humans.name}')
    scratch = output.parent / 'rfg_PC.partial'
    for path in (scratch, output):
        if path.exists():
            shutil.rmtree(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    str2 = output.parent / MEMBER
    str2.write_bytes(members[MEMBER])
    archive_v3.extract(str2, scratch)
    scratch.rename(output)
    str2.unlink()  # the extracted members are all that is used
    print(json.dumps(dict(output=output.name, members=len(list(output.iterdir())) - 1)))


if __name__ == '__main__':
    extract(Path(sys.argv[1]), Path(sys.argv[2]) if len(sys.argv) > 2 else PROJECT / 'LocalData/Extracted/Player/rfg_PC')
