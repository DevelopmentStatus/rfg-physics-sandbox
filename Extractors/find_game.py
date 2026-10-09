"""Print the folder of a Steam install of Red Faction Guerrilla (the one containing RFG.exe), or nothing.

Looks at the Steam install path in the registry, every library listed in libraryfolders.vdf, and
steamapps/appmanifest_20500.acf (the game's Steam app id) for its install folder name.
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


def libraries(root):
    found = [root]
    vdf = root / 'steamapps/libraryfolders.vdf'
    if vdf.is_file():
        for match in re.finditer(r'"path"\s+"([^"]+)"', vdf.read_text(errors='replace')):
            found.append(Path(match.group(1).replace(chr(92) * 2, chr(92))))
    return found


def main():
    for root in steam_roots():
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
