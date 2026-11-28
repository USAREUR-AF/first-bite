"""Tests of tools/bite2pack/embed.py: the model modules that are copied and
embedded are those the packs use. Python 3 standard library only.

    python3 -m unittest discover -s tools/bite2pack -p 'test_*.py'
"""
import importlib.util
import pathlib
import shutil
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("embed", HERE / "embed.py")
EMBED = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EMBED)


class EmbedTests(unittest.TestCase):
    def tree(self, files):
        root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text, encoding="utf-8")
        return root

    def test_every_module_a_pack_of_flow_xf_uses_is_there(self):
        self.assertEqual(EMBED.missing_modules(EMBED.XF), [])

    def test_the_models_copied_are_core_and_the_closure_of_the_packs_uses(self):
        root = self.tree(
            {
                "packs/cot-xml/pack.def": "format cot-xml\nuses cot\nuses track\n",
                "packs/geojson/pack.def": "format geojson\n",
                "model/core.def": "module core\n",
                "model/cot.def": "module cot\nuses timing\n",
                "model/timing.def": "module timing\n",
                "model/track.def": "module track\n",
                "model/radar.def": "module radar\n",
            }
        )
        self.assertEqual(
            EMBED.used_models(root, ["packs/cot-xml/pack.def", "packs/geojson/pack.def"]),
            ["model/core.def", "model/cot.def", "model/timing.def", "model/track.def"],
        )

    def test_a_module_a_pack_uses_and_flow_xf_lacks_is_named(self):
        root = self.tree(
            {
                "packs/cot-xml/pack.def": "format cot-xml\nuses cot\nuses track\n",
                "model/core.def": "module core\n",
                "model/track.def": "module track\n",
            }
        )
        self.assertEqual(
            EMBED.missing_modules(root), ["packs/cot-xml/pack.def: uses cot, and model/cot.def is not there"]
        )
        with self.assertRaises(SystemExit):
            EMBED.render(root)

    def test_each_model_comes_after_the_modules_it_uses(self):
        root = self.tree(
            {
                "packs/a/pack.def": "format a\nuses alpha\n",
                "model/core.def": "module core\n",
                "model/alpha.def": "module alpha\nuses zulu\n",
                "model/zulu.def": "module zulu\n",
            }
        )
        self.assertEqual([p.name for p in EMBED.model_order(root)], ["core.def", "zulu.def", "alpha.def"])


if __name__ == "__main__":
    unittest.main()
