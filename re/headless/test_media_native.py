"""Optional original-code scenario probes; all captured check inputs stay ignored.

Requires local media profile and CD-input capture in re/out/headless/. These
tests do not establish physical-media facts or boot-to-MainMenu acceptance.
"""

import copy
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from media import MediaProfile
from startup import OUTPUT_ROOT, REPO, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"
PROFILE = OUTPUT_ROOT / "hypothetical-cd-toc.json"
CAPTURE = OUTPUT_ROOT / "cd-inputs.json"


@unittest.skipUnless(
    EXE.exists() and PROFILE.exists() and CAPTURE.exists(),
    "local original and ignored hypothetical profile/CD-input vectors absent",
)
class OriginalMediaProbeTests(unittest.TestCase):
    def test_original_cd_predicate_acceptance_refusal_and_failure_paths(self):
        self.assertLessEqual(PROFILE.stat().st_size, 16384)
        self.assertLessEqual(CAPTURE.stat().st_size, 16384)
        source = json.loads(PROFILE.read_text())
        capture = json.loads(CAPTURE.read_text())
        count = capture["expected_track_count"]
        expected = capture["expected_words"][:count]
        self.assertTrue(type(count) is int and 1 <= count <= 128)
        self.assertEqual(len(expected), count)
        self.assertEqual(expected[0], count)
        self.assertTrue(all(type(v) is int and 0 <= v <= 0xFFFFFFFF for v in expected))
        cases = [("match", {}, 0)]
        tracks = source["track_lengths_ms"] + [50000]
        cases.append(
            (
                "count",
                {"track_lengths_ms": tracks, "total_ms": source["total_ms"] + 50000},
                0x150,
            )
        )
        tracks = source["track_lengths_ms"].copy()
        tracks[1] += 1
        cases.append(
            (
                "length",
                {"track_lengths_ms": tracks, "total_ms": source["total_ms"] + 1},
                0x150,
            )
        )
        for label, total, want in (
            ("low-bound", 4470000, 0),
            ("below-low", 4469999, 0x150),
            ("high-bound", 4830000, 0),
            ("above-high", 4830001, 0x150),
        ):
            tracks = source["track_lengths_ms"].copy()
            tracks[0] += total - source["total_ms"]
            cases.append((label, {"track_lengths_ms": tracks, "total_ms": total}, want))
        last = f"status:track:{len(source['track_lengths_ms'])}"
        for stage in (
            "open",
            "set",
            "status:count",
            "status:track:1",
            "status:track:2",
            last,
            "status:total",
            "close",
        ):
            cases.append(
                (
                    f"failure-{stage}",
                    {"failures": {stage: 276}},
                    0 if stage in ("status:track:1", last, "close") else 0x150,
                )
            )
        receipts = []
        for label, changes, want in cases:
            with self.subTest(case=label):
                data = copy.deepcopy(source)
                data.update(changes)
                run = Startup(EXE, media_profile=MediaProfile(data))
                argument = run.alloc(4 * count)
                run.write_u32s(argument, expected)
                result = run.call(0x401480, [argument], limit=100000)
                self.assertEqual(result.eax, want)
                receipts.append(
                    {
                        "case": label,
                        "eax": result.eax,
                        "instruction_counter": result.icount,
                    }
                )
        (OUTPUT_ROOT / "native-media-cases.json").write_text(
            json.dumps(receipts, indent=2) + "\n"
        )

    def test_declared_profile_prefix_is_repeatable_and_stops_before_d3d_success(self):
        profile = MediaProfile.load(PROFILE)
        first = Startup(EXE, media_profile=profile, record_limit=20000).run(
            limit=5000000
        )
        second = Startup(EXE, media_profile=profile, record_limit=20000).run(
            limit=5000000
        )
        self.assertEqual(first, second)
        self.assertEqual(first["stop"]["kind"], "unsupported_import")
        self.assertEqual(first["stop"]["name"], "d3d9.dll!Direct3DCreate9")
        self.assertEqual(first["media_scenario"], profile.metadata)
        self.assertEqual(first["event_limit"], 20000)
        self.assertTrue(first["winmain_reached"])
        self.assertFalse(first["main_menu_reached"])
        self.assertEqual(first["status"], "incomplete")


if __name__ == "__main__":
    unittest.main()
