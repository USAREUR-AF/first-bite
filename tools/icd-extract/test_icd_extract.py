#!/usr/bin/env python3
"""Tests for icd_extract.py. Standard library only.

    python3 tools/icd-extract/test_icd_extract.py

The first test needs no PDF reader. The second runs the helper on the worked
example's PDF and compares the draft with the reviewed tables; it is skipped
where `pdftotext` is not installed.
"""
import csv
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import icd_extract  # noqa: E402

EXAMPLE = HERE.parent.parent / "docs" / "icd-to-dictionary" / "06-worked-example"
NUMBERING = (16, "lsb", 1, 0)

LAYOUT = """\
4 Message header
Table 4-1: Message header

 Field             Word   Bits    Type   LSB    Units    Description

 Message ID        1      15-8    U      —      —        Identifies the message.

5 Reports
5.1 Messages
Table 5-1: Report messages

 ID     Message          Length (words)

 1      Vent Report      3

 2      Later            —

5.2 Vent Report
Table 5-2: Vent Report

 Field             Word   Bits    Type   LSB    Units    Description

 Windward Vent     2      15-8    U      0.5    %        Opening of the windward
 Position                                                vents.

 Vent Drive        2      7-      E      —      —        State of the drive. See Table 5-2a.
 State                    4

 Spare             2      3-0     —      —      —        Set to zero.

 Air Temperature   3      15-0    S      0.01   °C       Air temperature.


Table 5-2a: Vent Drive State values
 Value            Meaning

 0                Idle

 1                Opening

 2-15             Reserved

Text after the table.
"""


def rows(path):
    with open(path, encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


class Layout(unittest.TestCase):
    def test_wrapped_cells_header_rows_and_values(self):
        messages, fields, values, remarks = icd_extract.draft(LAYOUT, [("5", "RPT")], "4-1", NUMBERING)
        self.assertEqual(remarks, [])
        self.assertEqual(messages, [
            ["RPT", "Vent Report", "1", "yes", "", "", "5.2", "5-2", ""],
            ["RPT", "Later", "2", "no", "", "", "", "", ""],
        ])
        self.assertEqual([row[2:10] for row in fields], [
            ["message_id", "1", "15-8", "", "", "", "", "Identifies the message."],
            ["windward_vent_position", "2", "15-8", "", "0.5", "", "%", "Opening of the windward vents."],
            ["vent_drive_state", "2", "7-4", "", "", "", "", "State of the drive."],
            ["spare_28", "2", "3-0", "", "", "", "", ""],
            ["air_temperature", "3", "15-0", "int", "0.01", "", "degC", "Air temperature."],
        ])
        self.assertEqual([row[10:12] for row in fields][:2], [["4", "4-1"], ["", ""]])
        self.assertTrue(all(row[-1] == "unreviewed" for row in fields))
        self.assertEqual(values, [
            ["RPT", "Vent Report", "vent_drive_state", "0", "Idle"],
            ["RPT", "Vent Report", "vent_drive_state", "1", "Opening"],
        ])

    def test_a_table_outside_every_interface_is_remarked_on(self):
        _, fields, _, remarks = icd_extract.draft(LAYOUT, [("9", "X")], "", NUMBERING)
        self.assertEqual(fields, [])
        self.assertTrue(any("belongs to no --interface" in remark for remark in remarks), remarks)


@unittest.skipUnless(shutil.which("pdftotext"), "pdftotext is not installed")
class WorkedExample(unittest.TestCase):
    def test_draft_against_the_reviewed_tables(self):
        with tempfile.TemporaryDirectory() as out:
            code = icd_extract.main([
                str(EXAMPLE / "icd" / "GZC-ICD-001-B.pdf"), "--out", out,
                "--interface", "ZONE=5", "--interface", "SUPV=6", "--header-table", "4-1",
                "--word-bits", "16", "--bit0", "lsb", "--first-word", "1"])
            self.assertEqual(code, 0)
            draft, reviewed = rows(Path(out) / "fields.csv"), rows(EXAMPLE / "tables" / "fields.csv")
            draft_values = rows(Path(out) / "values.csv")
            draft_messages = rows(Path(out) / "messages.csv")

        def key(row):
            return row["interface"], row["message"], row["field"]

        by_key = {key(row): row for row in reviewed}
        self.assertEqual(len(draft), 115)
        self.assertEqual(len(reviewed), 116)
        self.assertTrue(all(row["review"] == "unreviewed" for row in draft))
        columns = [c for c in reviewed[0] if c != "note"]
        identical, differing = 0, {}
        for row in draft:
            other = by_key.get(key(row))
            if other is None:
                differing[key(row)] = ["no such field"]
                continue
            changed = [c for c in columns if row[c] != other[c]]
            if changed:
                differing[key(row)] = changed
            else:
                identical += 1
        # What the reviewer had to change: the unit of 36 dimensionless
        # fields, which take "1"; the offset of the offset-binary field; the
        # erratum of Table 5-3; and the sign-magnitude field, which becomes two.
        self.assertEqual(identical, 76)
        only_unit = [k for k, changed in differing.items() if changed == ["unit"]]
        self.assertEqual(len(only_unit), 36)
        rest = {k: v for k, v in differing.items() if v != ["unit"]}
        self.assertEqual(rest, {
            ("ZONE", "Climate Report", "leaf_wetness"): ["bits", "unit"],
            ("ZONE", "Climate Report", "outside_temperature"): ["offset"],
            ("ZONE", "Vent Report", "roof_vent_offset"): ["no such field"],
        })
        # Every enumerated value but those of the field the reviewer split.
        reviewed_values = rows(EXAMPLE / "tables" / "values.csv")
        self.assertEqual(len(draft_values), 28)
        self.assertEqual([v for v in draft_values if v not in reviewed_values], [])
        self.assertEqual([m["message"] for m in draft_messages if m["defined"] == "no"], ["Maintenance Data"])
        self.assertEqual([m["type_id"] for m in draft_messages], ["1", "2", "3", "4", "5", "6", "1", "2", "3", "4", "7"])


if __name__ == "__main__":
    unittest.main()
