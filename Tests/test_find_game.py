"""Steam discovery on macOS / Linux with synthetic Wine bottles; no game installation is needed."""
import contextlib
import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

PROJECT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT / 'Extractors'))
import find_game


def fake_game(folder):
    (folder / 'build/pc/cache').mkdir(parents=True)
    (folder / 'RFG.exe').write_bytes(b'synthetic executable')
    return folder


def discover(home, platform):
    out = io.StringIO()
    with patch.object(find_game.sys, 'platform', platform), patch.object(Path, 'home', return_value=home), \
            contextlib.redirect_stdout(out):
        find_game.main()
    return out.getvalue().strip()


class FindGameTests(unittest.TestCase):
    def test_crossover_bottle_default_library(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            steam = home / 'Library/Application Support/CrossOver/Bottles/Steam/drive_c/Program Files (x86)/Steam'
            game = fake_game(steam / 'steamapps/common/Red Faction Guerrilla')
            self.assertEqual(discover(home, 'darwin'), str(game))

    def test_whisky_bottle_library_on_another_drive(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            bottle = home / 'Library/Containers/com.isaacmarovitz.Whisky/Bottles/abc'
            apps = bottle / 'drive_c/Program Files (x86)/Steam/steamapps'
            apps.mkdir(parents=True)
            (apps / 'libraryfolders.vdf').write_text('"libraryfolders"\n{\n "1"\n {\n  "path"\t\t"D:\\\\Games"\n }\n}\n')
            library = bottle / 'drive_d/Games/steamapps'
            game = fake_game(library / 'common/RFG')
            (library / 'appmanifest_20500.acf').write_text('"AppState"\n{\n "installdir"\t\t"RFG"\n}\n')
            self.assertEqual(discover(home, 'darwin'), str(game))

    def test_linux_steam(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            game = fake_game(home / '.local/share/Steam/steamapps/common/Red Faction Guerrilla')
            self.assertEqual(discover(home, 'linux'), str(game))

    def test_nothing_found_prints_nothing(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(discover(Path(tmp), 'darwin'), '')


if __name__ == '__main__':
    unittest.main()
