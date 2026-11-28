"""Tests of the bindings of tools/bite2pack/bite2pack.py that a dictionary of this
repository does not use: an identifier that is a number, a track flag, a height in
whole metres, and a dictionary that is not public. Python 3 standard library only.

    python3 -m unittest discover -s tools/bite2pack -p 'test_*.py'
"""
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent

# An invented interface: a track with a number, a confirmed flag and a position
# whose height is whole metres, and a ping that carries the track number only.
METADATA = {
    "description": "invented for a test",
    "protocol": {"network": "ip", "transport": "udp"},
    "port": 40999,
    "message_type": {"start": 0, "stop": 8, "values": {"1": "Track", "2": "Ping"}},
}
TRACK = {
    "description": "a track",
    "fields": {
        "message_type": {"start": 0, "stop": 8, "values": {"1": "Track", "2": "Ping"}},
        "track_number": {"start": 8, "stop": 24, "unit": "1"},
        "confirmed": {"start": 24, "stop": 25, "type": "bool"},
        "spare_25": {"start": 25, "stop": 32},
        "latitude": {"start": 32, "stop": 64, "type": "int", "scale": 1e-07, "unit": "deg"},
        "longitude": {"start": 64, "stop": 96, "type": "int", "scale": 1e-07, "unit": "deg"},
        "altitude": {"start": 96, "stop": 112, "type": "int", "unit": "m"},
    },
}
PING = {
    "description": "a ping",
    "fields": {
        "message_type": {"start": 0, "stop": 8, "values": {"1": "Track", "2": "Ping"}},
        "track_number": {"start": 8, "stop": 24, "unit": "1"},
    },
}
TRACK_BINDING = {
    "uid": "track_number",
    "lat": "latitude",
    "lon": "longitude",
    "hae": "altitude",
    "track.confirmed": "confirmed",
}


class BindingTests(unittest.TestCase):
    def setUp(self):
        self.root = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        # the directory's name is not the dictionary's: the name in dictionary.json is used
        dictionary = self.root / "probe" / "dictionary"
        (dictionary / "PRB" / "messages").mkdir(parents=True)
        (dictionary / "dictionary.json").write_text(json.dumps({"name": "probe", "version": "0.1.0"}))
        (dictionary / "PRB" / "metadata.json").write_text(json.dumps(METADATA))
        (dictionary / "PRB" / "messages" / "Track.json").write_text(json.dumps(TRACK))
        (dictionary / "PRB" / "messages" / "Ping.json").write_text(json.dumps(PING))
        self.dictionary = dictionary

    def run_tool(self, bindings, *more):
        path = self.root / "bindings.json"
        path.write_text(json.dumps({"probe": bindings}))
        out = self.root / "out"
        return out, subprocess.run(
            [sys.executable, str(HERE / "bite2pack.py"), "--out", str(out), "--bindings", str(path)]
            + list(more) + [str(self.dictionary)],
            capture_output=True, text=True)

    def test_a_number_a_flag_and_whole_metres_are_bound_and_the_packs_are_controlled(self):
        out, result = self.run_tool({"PRB/Track": TRACK_BINDING, "PRB/Ping": {"uid": "track_number"}},
                                    "--distribution", "controlled")
        self.assertEqual(result.returncode, 0, result.stderr)
        module = (out / "model" / "probe.def").read_text()
        pack = (out / "packs" / "bite-probe-prb" / "pack.def").read_text()
        # the identifier is the track number's decimal text, derived for every message
        self.assertIn("derive  uid  text_of  probe.track_number\n", module)
        self.assertNotIn("field     uid", pack)
        # the flag is the bit, 0 or 1, of the track module
        self.assertIn("uses          track\n", pack)
        self.assertRegex(pack, r"\nfield +track\.confirmed +0:24:1 +uint optional\n")
        # whole metres, read as a number
        self.assertRegex(pack, r"\nfield +hae +0:96:16 +sscale:1 optional\n")
        # not public, and no licence claimed for a dictionary that states none
        for text in (module, pack):
            self.assertIn("distribution  controlled\n", text)
            self.assertNotIn("Apache", text)

    def test_an_identifier_derived_in_one_message_must_be_bound_in_every_message_with_the_field(self):
        _, result = self.run_tool({"PRB/Track": TRACK_BINDING})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("`uid` is derived from probe.track_number for every message that has it; "
                      "bind it in PRB/Ping too", result.stderr)

    def test_a_flag_needs_a_bool_field(self):
        _, result = self.run_tool({"PRB/Track": {"track.confirmed": "spare_25"}})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("`track.confirmed` is a flag; field `spare_25` is a uint, not a bool", result.stderr)

    def test_a_public_dictionary_keeps_its_licence(self):
        out, result = self.run_tool({"PRB/Track": TRACK_BINDING, "PRB/Ping": {"uid": "track_number"}})
        self.assertEqual(result.returncode, 0, result.stderr)
        pack = (out / "packs" / "bite-probe-prb" / "pack.def").read_text()
        self.assertIn("distribution  public\n", pack)
        self.assertIn("(Apache License 2.0)", pack)


if __name__ == "__main__":
    unittest.main()
