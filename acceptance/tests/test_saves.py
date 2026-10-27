"""Native save comparison and cross-load controls, without licensed data."""
from tests.support import *
import saves


class SavesTests(unittest.TestCase):
    """S8/G9 comparison over synthetic complete runs (data-free)."""

    EXE = "e" * 64

    def footer(self, body=b"\0\0\0\0"):
        # [slot count 0][resource count 0][total 8]
        return saves.SDAT_HEADER + body + (0).to_bytes(4, "little") * 2 + (8).to_bytes(4, "little")

    def build(self, root: Path):
        ids = sorted(saves.REQUIRED_SESSIONS)
        spec_sessions = []
        for sid in ids:
            ops = [{"op": "open", "file": "f", "name": "n", "mode": 1 if sid.endswith("write") else 0}]
            if sid.endswith("write") or sid == "native_arms":
                ops += [{"op": name, "file": "f"} for name in ("write_int", "write_float", "write_string", "write_res_ref")]
            else:
                ops += [{"op": name, "file": "f"} for name in ("read_int", "read_float", "read_string", "read_res_id")]
            ops.append({"op": "close", "file": "f"})
            session = {"id": sid, "ops": ops}
            if sid.endswith("_read") and not sid.startswith("stock"):
                session["seed"] = {"n": {"from": "session", "session": sid.replace("_read", "_write"), "name": "n"}}
            if sid.startswith("stock"):
                session["seed"] = {"n": {"from": "stock", "path": "x"}}
            spec_sessions.append(session)
        spec = {"schema": 1, "kinds": {}, "sessions": spec_sessions}
        spec_sha = "s" * 64
        files = {}
        for sid in ids:
            if sid.endswith("write"):
                word = saves.TYPE_WORDS.get(sid, 1).to_bytes(4, "little")
                files[sid] = self.footer(word)

        def run(implementation, seed_dir, name):
            directory = root / name
            sessions = {}
            for session in spec_sessions:
                sid = session["id"]
                ops = []
                for index, op in enumerate(session["ops"]):
                    entry = {"op": op["op"], "file": op["file"], "result": 7, "loads": [], "log": []}
                    if index == 0:
                        entry.update(result=1, handle=True, tables={"resources": [[1, 3, "a"]] * 3, "slots": [[1, 5, 0]] * 2})
                        if sid == "vehicle_read":
                            entry["loads"] = [["load_companion", "n.1"]]
                    if sid == "stock_options_read" and index == 1:
                        entry["result"] = saves.STOCK_OPTIONS_TYPE
                    ops.append(entry)
                written = {}
                if sid in files:
                    written = {"n": files[sid]}
                    path = directory / "files" / sid / "n"
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(files[sid])
                sessions[sid] = {"ops": ops, "files": {k: saves.sha256(v) for k, v in written.items()}}
                if "seed" in session:
                    sessions[sid]["files"]["n"] = "0" * 64
            results = {"schema": 1, "implementation": implementation, "spec_sha256": spec_sha, "sessions": sessions}
            if implementation == "original":
                results["exe_sha256"] = self.EXE
            directory.mkdir(exist_ok=True)
            (directory / "results.json").write_text(json.dumps(results))
            return {"results": results, "dir": directory}

        runs = {
            "original": run("original", None, "o"), "port": run("port", None, "p"),
            "original_cross": run("original", "p", "ox"), "port_cross": run("port", "o", "px"),
        }
        return spec, spec_sha, runs

    def check(self, mutate=None):
        with tempfile.TemporaryDirectory() as tmp:
            spec, spec_sha, runs = self.build(Path(tmp))
            if mutate:
                mutate(spec, runs, Path(tmp))
            return saves.compare(runs, spec, spec_sha, self.EXE)

    def test_complete_equal_runs_compare(self):
        summary = self.check()
        self.assertEqual(summary["sessions"], len(saves.REQUIRED_SESSIONS))
        self.assertGreater(summary["files_compared"], 2)

    def rejects(self, mutate, text=None):
        with self.assertRaises(ValueError) as context:
            self.check(mutate)
        if text:
            self.assertIn(text, str(context.exception))

    def test_changed_file_bytes_are_rejected(self):
        def mutate(spec, runs, root):
            path = runs["port"]["dir"] / "files/options_write/n"
            data = bytearray(path.read_bytes())
            data[8] ^= 1
            path.write_bytes(bytes(data))
        self.rejects(mutate)

    def test_recorded_hash_cannot_replace_the_file(self):
        def mutate(spec, runs, root):
            (runs["port"]["dir"] / "files/options_write/n").write_bytes(self.footer(b"\1\0\0\0"))
        self.rejects(mutate, "recorded hash")

    def test_missing_written_file_is_rejected(self):
        self.rejects(lambda spec, runs, root: (runs["original"]["dir"] / "files/vehicle_write/n").unlink(), "missing")

    def test_differing_operation_result_is_rejected(self):
        def mutate(spec, runs, root):
            runs["port"]["results"]["sessions"]["options_write"]["ops"][2]["result"] = 8
        self.rejects(mutate, "differs between original and port")

    def test_dropped_operation_is_rejected(self):
        def mutate(spec, runs, root):
            runs["port"]["results"]["sessions"]["options_write"]["ops"].pop()
        self.rejects(mutate, "dropped or extra")

    def test_reordered_operation_is_rejected(self):
        def mutate(spec, runs, root):
            ops = runs["port"]["results"]["sessions"]["options_write"]["ops"]
            ops[1], ops[2] = ops[2], ops[1]
        self.rejects(mutate, "not the scripted one")

    def test_other_spec_or_executable_is_rejected(self):
        self.rejects(lambda spec, runs, root: runs["port"]["results"].update(spec_sha256="x" * 64), "different spec")
        self.rejects(lambda spec, runs, root: runs["original"]["results"].update(exe_sha256="x" * 64), "build-606")
        self.rejects(lambda spec, runs, root: runs["port"]["results"].update(implementation="original"), "implementation")

    def test_spec_must_cover_every_native_and_required_session(self):
        def drop_native(spec, runs, root):
            for session in spec["sessions"]:
                session["ops"] = [op for op in session["ops"] if op["op"] != "read_res_id"]
        self.rejects(drop_native, "never calls")
        self.rejects(lambda spec, runs, root: spec["sessions"].pop(0), "lacks sessions")

    def test_cross_load_mismatch_in_either_direction_is_rejected(self):
        for name in ("original_cross", "port_cross"):
            with self.subTest(name=name):
                def mutate(spec, runs, root, name=name):
                    runs[name]["results"]["sessions"]["options_read"]["ops"][3]["result"] = 99
                self.rejects(mutate, "cross-load")

    def test_empty_reads_and_missing_tables_cannot_pass(self):
        def empty(spec, runs, root):
            for name in runs:
                for op in runs[name]["results"]["sessions"]["options_read"]["ops"][1:]:
                    op["result"] = 0
        self.rejects(empty, "empty")
        def no_tables(spec, runs, root):
            for name in ("original", "port", "original_cross", "port_cross"):
                runs[name]["results"]["sessions"]["vehicle_read"]["ops"][0]["tables"] = {"resources": [], "slots": []}
        self.rejects(no_tables, "rebuilt no tables")

    def test_malformed_sdat_is_rejected(self):
        for sid in ("options_write", "vehicle_write"):
            with self.subTest(sid=sid):
                def mutate(spec, runs, root, sid=sid):
                    for name in ("original", "port"):
                        path = runs[name]["dir"] / "files" / sid / "n"
                        path.write_bytes(b"JSON" + path.read_bytes()[4:])
                        runs[name]["results"]["sessions"][sid]["files"] = {"n": saves.sha256(path.read_bytes())}
                self.rejects(mutate, "SDAT header")

    def test_wrong_type_word_and_bad_footer_total_are_rejected(self):
        def type_word(spec, runs, root):
            for name in ("original", "port"):
                path = runs[name]["dir"] / "files/vehicle_write/n"
                path.write_bytes(self.footer(b"\2\0\0\0"))
                runs[name]["results"]["sessions"]["vehicle_write"]["files"] = {"n": saves.sha256(path.read_bytes())}
        self.rejects(type_word, "type word")
        def total(spec, runs, root):
            for name in ("original", "port"):
                path = runs[name]["dir"] / "files/options_write/n"
                data = path.read_bytes()[:-4] + (999).to_bytes(4, "little")
                path.write_bytes(data)
                runs[name]["results"]["sessions"]["options_write"]["files"] = {"n": saves.sha256(data)}
        self.rejects(total, "footer total")

    def test_g9_is_blocked_even_when_native_level_sessions_pass(self):
        passed = saves.result("passed", "x")
        gate = saves.gate_result(passed)
        self.assertEqual(gate["result"], "blocked")
        self.assertEqual(saves.gate_result(None)["result"], "blocked")
        self.assertTrue(any("JPEG" in text for text in gate["blockers"]))

    def test_inventory_never_reports_these_rows_passed_without_execution(self):
        data = copy.deepcopy(BASE)
        for row in data["rows"]:
            row["status"], row["evidence"] = "open", []
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "matrix.json"
            path.write_text(json.dumps(data))
            output = subprocess.run([sys.executable, str(CHECK), "--matrix", str(path), "--table", "--json"],
                                    capture_output=True, text=True)
        report = {row["id"]: row for row in json.loads(output.stdout)["results"]}
        self.assertEqual(report["S8"]["result"], "not_run")
        self.assertEqual(report["G9"]["result"], "blocked")

