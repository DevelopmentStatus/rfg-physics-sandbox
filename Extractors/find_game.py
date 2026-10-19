"""Print the folder of a Steam install of Red Faction Guerrilla (the one containing RFG.exe), or nothing.

Looks at the Steam install path in the registry, every library listed in libraryfolders.vdf, and
steamapps/appmanifest_20500.acf (the game's Steam app id) for its install folder name.
On macOS and Linux it also checks the native Steam folders and the Windows Steam inside CrossOver,
Whisky and Wine prefixes (the game itself is Windows-only, so it is usually installed through one).
"""
import re
import sys
from pathlib import Path

APP_ID = '20500'


def steam_roots():
    roots = []
    if sys.platform == 'win32':
        import winreg
        for hive, key, value in ((winreg.HKEY_CURRENT_USER, r'Software\Valve\Steam', 'SteamPath'),
                                 (winreg.HKEY_LOCAL_MACHINE, r'SOFTWARE\WOW6432Node\Valve\Steam', 'InstallPath'),
                                 (winreg.HKEY_LOCAL_MACHINE, r'SOFTWARE\Valve\Steam', 'InstallPath')):
            try:
                with winreg.OpenKey(hive, key) as k:
                    roots.append(Path(winreg.QueryValueEx(k, value)[0]))
            except OSError:
                pass
        roots += [Path(p) / 'Steam' for p in ('C:/Program Files (x86)', 'C:/Program Files')]
        return roots
    home = Path.home()
    if sys.platform == 'darwin':
        roots.append(home / 'Library/Application Support/Steam')
        prefixes = [*(home / 'Library/Application Support/CrossOver/Bottles').glob('*'),
                    *(home / 'Library/Containers/com.isaacmarovitz.Whisky/Bottles').glob('*')]
    else:
        roots += [home / '.steam/steam', home / '.local/share/Steam',
                  home / '.var/app/com.valvesoftware.Steam/.local/share/Steam']
        prefixes = []
    prefixes.append(home / '.wine')
    for prefix in prefixes:
        roots += [prefix / 'drive_c' / p / 'Steam' for p in ('Program Files (x86)', 'Program Files')]
    return roots


def native_path(path, root):
    """Map a Windows library path ("D:\\Games") from a Wine prefix's vdf onto that prefix's drive folders."""
    m = re.match(r'^([A-Za-z]):[\\/](.*)$', path)
    if not m or sys.platform == 'win32':
        return Path(path)
    for parent in root.parents:
        if parent.name == 'drive_c':
            drive = parent.parent / 'dosdevices' / f'{m.group(1).lower()}:'
            if m.group(1).lower() == 'c' or not drive.exists():
                drive = parent.parent / f'drive_{m.group(1).lower()}'
            return drive / m.group(2).replace(chr(92), '/')
    return Path(path)


def libraries(root):
    found = [root]
    vdf = root / 'steamapps/libraryfolders.vdf'
    if vdf.is_file():
        for match in re.finditer(r'"path"\s+"([^"]+)"', vdf.read_text(errors='replace')):
            found.append(native_path(match.group(1).replace(chr(92) * 2, chr(92)), root))
    return found


def main():
    for root in steam_roots():
        if not root.is_dir():
            continue
        for library in libraries(root):
            apps = library / 'steamapps'
            names = ['Red Faction Guerrilla']
            manifest = apps / f'appmanifest_{APP_ID}.acf'
            if manifest.is_file():
                m = re.search(r'"installdir"\s+"([^"]+)"', manifest.read_text(errors='replace'))
                if m:
                    names.insert(0, m.group(1))
            for name in names:
                game = apps / 'common' / name
                if (game / 'RFG.exe').is_file():
                    print(game)
                    return


if __name__ == '__main__':
    main()
