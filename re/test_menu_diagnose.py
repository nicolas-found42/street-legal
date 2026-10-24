"""Real child-process tests for session evidence, without licensed data or GPU."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('menu', Path(__file__).with_name('menu-diagnose.py'))
menu = importlib.util.module_from_spec(spec)
spec.loader.exec_module(menu)
FAKE = r'''#!/usr/bin/env python3
import json, os, pathlib, sys, time
args=sys.argv[1:]; out=pathlib.Path(args[args.index('--output')+1]); rid=os.environ['SLR_SESSION_RUN_ID']
if '--hang' in args: time.sleep(60)
seq=0
with (out/'session-00000.jsonl').open('w') as log:
 def emit(kind,data):
  global seq
  seq+=1;log.write(json.dumps({'schema_version':1,'run_id':rid,'sequence':seq,'wall_ns':seq,'context':{'frame':7},'kind':kind,'data':data})+'\n');log.flush()
 emit('menu-state',{'state':{'focus':1,'selected':0,'labels':['MAIN MENU'],'exit':False}})
 emit('render-state',{'recipe_sha256':'initial'})
 tape=json.loads(pathlib.Path(args[args.index('--input-script')+1]).read_text()) if '--input-script' in args else [{'tick':5,'event':{'type':'key','code':13,'pressed':True}},{'tick':5,'event':{'type':'key','code':13,'pressed':False}}]
 for e in tape:
  emit('input-apply',e);i=seq
  if '--crash' in args: sys.exit(3)
  emit('input-result',{'input_id':i,'error':None})
 emit('menu-state',{'state':{'focus':2,'selected':0,'labels':['OPTIONS'],'exit':False}})
 emit('render-state',{'recipe_sha256':'changed' if '--changed' in args else 'options'})
 if '--window' in args:
  emit('frame-presented',{'frame_id':8});emit('ui-exit',{'error':None})
(out/'capture-end.json').write_text(json.dumps({'status':'completed'}))
'''

class SessionTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)
        self.previous=menu.REPO;menu.REPO=self.root
        (self.root/'stock').mkdir();(self.root/'stock/data').write_text('licensed fixture')
        (self.root/'.gitignore').write_text('runs/\n__pycache__/\n')
        self.binary=self.root/'fake';self.binary.write_text(FAKE);self.binary.chmod(0o755)
        subprocess.run(['git','init','-q',str(self.root)],check=True)
        subprocess.run(['git','-C',str(self.root),'add','.'],check=True)
        subprocess.run(['git','-C',str(self.root),'-c','user.name=Test','-c','user.email=test@example.test','commit','-qm','fixture'],check=True)
        self.args=argparse.Namespace(binary=self.binary,runs_root=self.root/'runs',headless=True,input_script=None,timeout=5,stall_timeout=2,full=False,expect=None)
    def tearDown(self): menu.REPO=self.previous;self.temp.cleanup()
    def run_case(self,extra=(),prior=None):return menu.capture(self.args,['stock',*extra],prior)
    def test_capture_freezes_complete_same_frame_tape_and_provenance(self):
        code,bundle=self.run_case()
        self.assertEqual(code,0)
        value=menu.manifest(bundle)
        self.assertEqual(value['embedding'],'slr-menu')
        self.assertTrue(value['source']['commit']);self.assertTrue(value['binary']['sha256'])
        tape=json.loads((bundle/'inputs/recorded-input.json').read_text())
        self.assertEqual(len(tape),2);self.assertEqual(tape[0]['tick'],tape[1]['tick'])
        menu.boot.verify_artifacts(bundle,value)
        self.assertEqual(len(menu.boot.read_index(self.args.runs_root)),1)
    def test_frozen_script_survives_original_deletion_and_verification_detects_changed_recipe(self):
        script=self.root/'runs-script.json';script.write_text('[{"tick":3,"event":{"type":"text","text":"AA"}}]')
        self.args.input_script=script
        code,first=self.run_case();self.assertEqual(code,0)
        script.unlink()
        self.args.input_script=first/'inputs/recorded-input.json'
        code,second=self.run_case(prior=first);self.assertEqual(code,0)
        self.assertTrue(json.loads((second/'report.json').read_text())['verification']['passed'])
        code,third=self.run_case(['--changed'],first);self.assertEqual(code,1)
        report=json.loads((third/'report.json').read_text())
        self.assertEqual(report['verification']['mismatches']['renders']['transition'],1)
    def test_failure_before_result_retains_input_and_pins_bundle(self):
        code,bundle=self.run_case(['--crash']);self.assertEqual(code,1)
        self.assertTrue(menu.manifest(bundle)['pinned'])
        self.assertEqual(len(menu.behavior(bundle)['pending_input_ids']),1)
        result=menu.prune(self.args.runs_root,1,1)
        self.assertTrue(result['budget_blocked_by_protected_runs']);self.assertTrue(bundle.exists())
    def test_heartbeat_timeout_terminates_child_and_registers_failure(self):
        self.args.stall_timeout=0.2
        code,bundle=self.run_case(['--hang']);self.assertEqual(code,1)
        report=json.loads((bundle/'report.json').read_text())
        self.assertIn('heartbeat-timeout',report['termination'])
        self.assertEqual(menu.recover(self.args.runs_root)[-1]['status'],'failed')
    def test_tampering_is_rejected_and_partial_tail_is_diagnostic_only(self):
        _,bundle=self.run_case()
        path=bundle/'runtime/session-00000.jsonl'
        with path.open('a') as f:f.write('{')
        with self.assertRaises(ValueError):menu.boot.verify_artifacts(bundle,menu.manifest(bundle))
        self.assertTrue(list(menu.records(bundle,True)))
        with self.assertRaises(ValueError):list(menu.records(bundle))
    def test_stock_changes_prevent_historical_verification(self):
        _,bundle=self.run_case()
        (self.root/'stock/data').write_text('different')
        self.args.input_script=bundle/'inputs/recorded-input.json'
        code,new=self.run_case(prior=bundle);self.assertEqual(code,1)
        self.assertIn('stock differs',json.loads((new/'report.json').read_text())['error'])
    def test_fixed_callback_verifies_historical_successful_prefix(self):
        code,failed=self.run_case(['--crash']);self.assertEqual(code,1)
        self.args.input_script=failed/'inputs/recorded-input.json'
        code,fixed=self.run_case(prior=failed);self.assertEqual(code,0)
        verdict=json.loads((fixed/'report.json').read_text())['verification']
        self.assertEqual(verdict['policy'],'prior-successful-prefix-and-clean-completion')
    def test_explicit_final_assertion_must_pass(self):
        expected=self.root/'expect.json';expected.write_text('{"exit":true}')
        self.args.expect=expected
        code,bundle=self.run_case();self.assertEqual(code,1)
        self.assertFalse(json.loads((bundle/'report.json').read_text())['assertions']['passed'])
    def test_orphaned_supervisor_is_discoverable_and_protected(self):
        _,bundle=self.run_case()
        value=menu.manifest(bundle);value['status']='running';value['supervisor']={'pid':99999999,'identity':'dead process'}
        menu.boot.dump(bundle/'manifest.json',value)
        row=menu.recover(self.args.runs_root)[-1]
        self.assertEqual(row['status'],'interrupted');self.assertTrue(row['pinned'])
        self.assertFalse(menu.prune(self.args.runs_root,1,1)['removed'])
    def test_external_output_is_indexed_but_not_automatically_pruned(self):
        output=self.root/'custom-bundle'
        code,bundle=self.run_case(['--output',str(output)]);self.assertEqual(code,0)
        self.assertEqual(len(menu.recover(self.args.runs_root)),1)
        self.assertEqual(menu.prune(self.args.runs_root,1,1)['removed'],[])
        self.assertTrue(bundle.exists())
    def test_verified_local_export_preserves_bundle_and_rejects_self_archive(self):
        _,bundle=self.run_case()
        destination=self.root/'incident.tar.gz'
        self.assertEqual(menu.main(['export',str(bundle),str(destination)]),0)
        import tarfile
        with tarfile.open(destination) as archive:
            self.assertIn(bundle.name+'/manifest.json',archive.getnames())
        with self.assertRaises(ValueError):menu.main(['export',str(bundle),str(bundle/'bad.tar.gz')])
    def test_retention_preserves_index_after_pruning_successes(self):
        _,first=self.run_case();_,second=self.run_case()
        result=menu.prune(self.args.runs_root,1,2**30)
        self.assertEqual(result['removed'],[str(first)]);self.assertTrue(second.exists())
        self.assertEqual(len(menu.boot.read_index(self.args.runs_root)),2)

if __name__=='__main__':unittest.main()
