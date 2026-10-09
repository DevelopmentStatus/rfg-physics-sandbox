"""Regression checks with synthetic files; no game installation is needed."""
import contextlib
import io
import json
from pathlib import Path
import sys
import struct
import tempfile
import unittest
from unittest.mock import patch

PROJECT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PROJECT / 'Extractors'))
import asset_status
import build_assets
import build_chunks
import extract_all


class ExtractionTests(unittest.TestCase):
    def setUp(self):
        self.test_root = PROJECT / 'LocalData/Tests'
        self.test_root.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=self.test_root)
        self.root = Path(self.temp.name)
        self.assets = self.root / 'LocalData/Assets'
        self.extracted = self.root / 'LocalData/Extracted'
        self.output = io.StringIO()
        self.capture = contextlib.redirect_stdout(self.output)
        self.capture.__enter__()

    def tearDown(self):
        self.capture.__exit__(None, None, None)
        # Only remove the scratch directory created by this test.
        assert self.root.resolve().is_relative_to(self.test_root.resolve())
        self.temp.cleanup()

    def write(self, relative, value):
        path = self.assets / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if path.suffix == '.glb':
            payload = b'{"asset":{"version":"2.0"}}  '
            path.write_bytes(struct.pack('<4sIII4s', b'glTF', 2, 20 + len(payload), len(payload), b'JSON') + payload)
        else:
            path.write_text(json.dumps(value) if not isinstance(value, str) else value)

    def ready_assets(self):
        self.write('mason.glb', 'synthetic model')
        weapons = {}
        for key in asset_status.WEAPONS:
            filename = f'weapons/{key}.glb'
            self.write(filename, 'synthetic model')
            weapons[key] = {'file': filename}
        self.write('weapons.json', weapons)
        self.write('camera_shakes.json', {'test': {}})
        self.write('chunks/materials.json', [{'name': 'test'}])
        self.write('chunks/catalog.json', [{'name': 'good'}])
        self.write('chunks/good.json', {'name': 'good', 'destroyables': []})
        self.write('chunks/good.glb', 'synthetic model')

    def ready_inputs(self):
        for name in ('MiscV3', 'Items', 'AnimsV3', 'Player/rfg_PC'):
            path = self.extracted / name
            path.mkdir(parents=True, exist_ok=True)
            (path / 'extraction_manifest.json').write_text('{}')
        chunks = self.extracted / 'Chunks'
        chunks.mkdir(parents=True, exist_ok=True)
        (chunks / 'index.json').write_text(json.dumps({'chunks': {'good.cchk_pc': {}}}))
        game = self.root / 'Game'
        (game / 'build/pc/cache').mkdir(parents=True)
        (game / 'RFG.exe').write_bytes(b'synthetic executable')
        return game

    def driver(self, *options):
        game = self.ready_inputs()
        stack = contextlib.ExitStack()
        stack.enter_context(patch.object(extract_all, 'ASSETS', self.assets))
        stack.enter_context(patch.object(extract_all, 'EXTRACTED', self.extracted))
        stack.enter_context(patch.object(extract_all, 'exe_hash', return_value=extract_all.EXPECTED_EXE_SHA256))
        stack.enter_context(patch.object(sys, 'argv', ['extract_all.py', '--game', str(game), *options]))
        return stack

    def test_complete_assets_pass(self):
        self.ready_assets()
        self.assertEqual(asset_status.asset_errors(self.assets, ['good']), [])

    def test_missing_weapon_is_not_complete(self):
        self.ready_assets()
        (self.assets / 'weapons/sledgehammer.glb').unlink()
        self.assertTrue(asset_status.weapon_errors(self.assets))

    def test_missing_chunk_is_retried(self):
        self.ready_assets()
        (self.assets / 'chunks/good.glb').unlink()
        self.assertEqual(asset_status.complete_chunks(self.assets), set())
        self.assertTrue(asset_status.asset_errors(self.assets, ['good']))

    def test_corrupt_chunk_definition_is_retried(self):
        self.ready_assets()
        self.write('chunks/good.json', '{broken')
        self.assertEqual(asset_status.complete_chunks(self.assets), set())

    def test_truncated_model_is_retried(self):
        self.ready_assets()
        model = self.assets / 'chunks/good.glb'
        model.write_bytes(model.read_bytes()[:-1])
        self.assertEqual(asset_status.complete_chunks(self.assets), set())

    def test_truncated_character_requires_rebuild(self):
        self.ready_assets()
        (self.assets / 'mason.glb').write_bytes(b'glTF')
        self.assertTrue(asset_status.weapon_errors(self.assets))

    def test_empty_catalog_fails(self):
        self.ready_assets()
        self.write('chunks/catalog.json', [])
        self.assertTrue(asset_status.asset_errors(self.assets))

    def test_duplicate_catalog_entries_fail(self):
        self.ready_assets()
        self.write('chunks/catalog.json', [{'name': 'good'}, {'name': 'good'}])
        self.assertTrue(asset_status.asset_errors(self.assets))

    def test_requested_chunk_must_be_in_catalog(self):
        self.ready_assets()
        self.assertTrue(asset_status.asset_errors(self.assets, ['missing']))

    def test_chunk_failure_preserves_success_and_removes_stale_entry(self):
        source = self.extracted / 'Chunks'
        source.mkdir(parents=True)
        (source / 'index.json').write_text(json.dumps({'chunks': {
            'good.cchk_pc': {'source': 'synthetic'}, 'bad.cchk_pc': {'source': 'synthetic'}}}))
        out = self.assets / 'chunks'
        out.mkdir(parents=True)
        (out / 'catalog.json').write_text('[{"name":"bad"}]')

        def convert(name, *_):
            if name == 'bad':
                raise ValueError('broken input')
            return {'name': name, 'destroyables': 1, 'pieces': 1, 'links': 0}

        with patch.multiple(build_chunks, PROJECT=self.root, SOURCE=source, OUT=out), \
                patch.object(build_chunks, 'read_materials', return_value=[]), \
                patch.object(build_chunks, 'Textures'), \
                patch.object(build_chunks, 'convert', side_effect=convert), \
                patch.object(sys, 'argv', ['build_chunks.py', 'good', 'bad']):
            with self.assertRaises(SystemExit):
                build_chunks.main()
        self.assertEqual([c['name'] for c in json.loads((out / 'catalog.json').read_text())], ['good'])

    def test_weapon_failure_preserves_success_and_fails_step(self):
        misc = self.extracted / 'MiscV3'
        misc.mkdir(parents=True)
        (misc / 'mason.rig_pc').write_bytes(b'synthetic rig')

        def build(key, *_):
            if key == 'bad':
                raise ValueError('broken input')
            return b'synthetic model', {'file': f'weapons/{key}.glb', 'min': [0, 0, 0], 'max': [1, 1, 1]}

        with patch.multiple(build_assets, PROJECT=self.root, EXTRACTED=self.extracted,
                            OUT=self.assets, WEAPONS={'good': (), 'bad': ()}), \
                patch.object(build_assets, 'build_mason', return_value=(b'model', [])), \
                patch.object(build_assets.rfg_model, 'read_rig', return_value=[]), \
                patch.object(build_assets.rfg_model, 'read_rig_tags', return_value=[]), \
                patch.object(build_assets, 'build_weapon', side_effect=build):
            with self.assertRaises(SystemExit):
                build_assets.main()
        self.assertEqual(set(json.loads((self.assets / 'weapons.json').read_text())), {'good'})
        self.assertTrue((self.assets / 'weapons/good.glb').is_file())

    def test_partial_steps_do_not_claim_full_readiness(self):
        with self.driver('--only', 'misc'):
            extract_all.main()
        self.assertIn('Selected steps completed', self.output.getvalue())
        self.assertNotIn('All requested assets verified', self.output.getvalue())

    def test_failed_verification_keeps_intermediates(self):
        self.ready_assets()
        (self.assets / 'chunks/good.glb').unlink()
        with self.driver('--clean'), patch.object(extract_all, 'run'), \
                patch.object(extract_all.shutil, 'rmtree') as cleanup:
            with self.assertRaises(SystemExit):
                extract_all.main()
            cleanup.assert_not_called()
        self.assertNotIn('All requested assets verified', self.output.getvalue())

    def test_full_complete_run_verifies_without_rebuilding(self):
        self.ready_assets()
        with self.driver(), patch.object(extract_all, 'run') as run:
            extract_all.main()
            run.assert_not_called()
        self.assertIn('All requested assets verified', self.output.getvalue())

    def test_cleanup_rejects_partial_request(self):
        with patch.object(sys, 'argv', ['extract_all.py', '--clean', '--chunks', 'good']), \
                contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                extract_all.main()
        self.assertEqual(error.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
