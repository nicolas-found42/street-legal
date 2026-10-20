"""End-to-end acceptance and failure tests for unattended boot diagnostics."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
COMMAND = ROOT / 're/boot-diagnose.py'
spec = importlib.util.spec_from_file_location('diagnose', COMMAND)
diag = importlib.util.module_from_spec(spec)
spec.loader.exec_module(diag)

FAKE = '''#!/usr/bin/env python3
import json,os,pathlib,subprocess,sys,time
args=sys.argv
out=pathlib.Path(args[args.index('--output')+1]);out.mkdir()
rid=args[args.index('--run-id')+1]
mode=next((x.split('=',1)[1] for x in args if x.startswith('--mode=')),'equal')
if mode in ('sleep','spawn'):
 subprocess.Popen([sys.executable,'-c',"import time,pathlib;time.sleep(2);pathlib.Path('escaped').write_text('bad')"])
 if mode=='sleep': time.sleep(30)
data=bytes.fromhex('FIXTURE_HEX')
if mode in ('mismatch','unstable'): data=data[:-4]+(8 if mode=='mismatch' or '--snapshot-native' not in args else 9).to_bytes(4,'little')
if mode=='short': data=data[:8]
if mode=='malformed': data=data[:-1]
for name in ['runtime.calllog','runtime-all.calllog']:
 p=out/name;p.write_bytes(data)
 if mode=='stale': os.utime(p,(1,1))
(out/'capture.json').write_text(json.dumps({'schema_version':1,'run_id': 'wrong' if mode=='wrong-id' else rid}))
(out/'capture-end.json').write_text(json.dumps({'schema_version':1,'run_id':rid,'events_written':3,'states_written':1,'last_sequence':11,'status':'completed','truncated':mode=='truncated','pending_records':0,'filtered_records':0 if mode=='short' else 1,'all_records':0 if mode=='short' else 1}))
event={'schema_version':1,'run_id':rid,'sequence':10,'context':{'native':{'call_id':55}},'kind':{'event':'external','category':'native-call','fields':[['filtered_ordinal','0'],['all_ordinal','0']]}}
if mode=='event-id': event['run_id']='wrong'
if '--clock-replay' in args:
 tape=pathlib.Path(args[args.index('--clock-replay')+1]);assert json.loads((tape.parent/'clock-verification.json').read_text())['fixture']==1;assert (tape.parent/'observed/original-facts.json').exists()
(out/'events.jsonl').write_text('broken' if mode=='bad-events' else json.dumps({'schema_version':1,'run_id':rid,'sequence':9,'kind':{'event':'external','category':'state-checkpoint'}})+'\\n'+json.dumps(event)+'\\n'+json.dumps({'schema_version':1,'run_id':rid,'sequence':11,'kind':{'event':'external','category':'capture-end'}})+'\\n')
if '--snapshot-native' in args: (out/'divergence.json').write_text(json.dumps({'schema_version':1,'run_id':rid,'native_call_id':55}))
(out/'states.jsonl').write_text(json.dumps({'schema_version':1,'sequence':9,'provenance':{'run_id':rid},'queues':{'ready':[1]}})+'\\n')
if mode=='empty-states': (out/'states.jsonl').write_text('')
if mode=='missing-events': (out/'events.jsonl').unlink()
'''

class RunTests(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory();self.repo=Path(self.temp.name)
  (self.repo/'stock').mkdir();(self.repo/'stock/a').write_bytes(b'stock')
  (self.repo/'.gitignore').write_text('runs/\nescaped\n')
  writer=diag.calllog.Writer(self.repo/'expected.slrlog');writer.record(diag.calllog.native_call('test.A','f',[],7));writer.close()
  (self.repo/'all.slrlog').write_bytes((self.repo/'expected.slrlog').read_bytes())
  (self.repo/'native.tsv').write_text('test.A\tf\t()I\n')
  binary=self.repo/'fake';binary.write_text(FAKE.replace('FIXTURE_HEX',(self.repo/'expected.slrlog').read_bytes().hex()));binary.chmod(0o755)
  subprocess.run(['git','init','-q',str(self.repo)],check=True)
  subprocess.run(['git','-C',str(self.repo),'add','.'],check=True)
  subprocess.run(['git','-C',str(self.repo),'-c','user.name=Test','-c','user.email=test@example.test','commit','-qm','fixture'],check=True)
 def tearDown(self): self.temp.cleanup()
 def argv(self,mode='equal',timeout=5):
  return [sys.executable,str(COMMAND),str(self.repo/'expected.slrlog'),'--oracle-all',str(self.repo/'all.slrlog'),'--repository',str(self.repo),'--native-api','native.tsv','--runs-root',str(self.repo/'runs'),'--binary','fake','--no-build','--timeout',str(timeout),'--','--mode='+mode]
 def invoke(self,mode='equal',timeout=5):
  result=subprocess.run(self.argv(mode,timeout),capture_output=True,text=True)
  bundle=next(iter((self.repo/'runs').iterdir()))
  return result,bundle,json.loads((bundle/'manifest.json').read_text()),json.loads((bundle/'report.json').read_text())
 def test_equal_complete_and_immutable_analysis(self):
  result,bundle,manifest,report=self.invoke()
  self.assertEqual(result.returncode,0,result.stderr+result.stdout)
  self.assertEqual(manifest['status'],'completed');self.assertTrue(report['comparisons']['unfiltered']['equal'])
  self.assertEqual(diag.analyze(bundle)['status'],'completed')
  (bundle/'runtime/runtime.calllog').write_bytes(b'changed')
  with self.assertRaisesRegex(ValueError,'artifact changed'):diag.analyze(bundle)
 def test_fail_closed_cases(self):
  for mode in ['short','malformed','stale','wrong-id','truncated','bad-events','event-id','empty-states','missing-events']:
   with self.subTest(mode=mode):
    result,bundle,manifest,report=self.invoke(mode)
    self.assertNotEqual(result.returncode,0);self.assertEqual(manifest['status'],'failed')
    if mode=='short':
     self.assertEqual(report['comparisons']['filtered']['kind'],'length_mismatch')
     self.assertEqual(report['divergence_context']['event']['sequence'],10)
     self.assertEqual(report['nearest_preceding_state']['sequence'],9)
    import shutil;shutil.rmtree(self.repo/'runs')
 def test_targeted_rerun_proves_full_history(self):
  _,bundle,manifest,report=self.invoke('mismatch')
  self.assertTrue(report['targeted_rerun']['snapshot_valid'])
  self.assertTrue(report['targeted_rerun']['identical_full_traces'])
  self.assertEqual(report['targeted_rerun']['native_call_id'],55)
 def test_targeted_rerun_rejects_nondeterminism(self):
  _,_,_,report=self.invoke('unstable')
  self.assertFalse(report['targeted_rerun']['snapshot_valid'])
  self.assertFalse(report['targeted_rerun']['identical_full_traces'])
 def test_regression_command_rechecks_fixed_frontier(self):
  _,bundle,_,_=self.invoke('mismatch')
  binary=self.repo/'fake';binary.write_text(binary.read_text().replace("('mismatch','unstable')", "('unstable',)"))
  result=subprocess.run(self.argv()[:-2]+['--check-regression',str(bundle)],capture_output=True,text=True)
  self.assertEqual(result.returncode,0,result.stdout+result.stderr)
  newest=max((self.repo/'runs').iterdir(),key=lambda p:p.name)
  self.assertTrue(json.loads((newest/'report.json').read_text())['regression_check']['passed'])
 def test_earliest_unfiltered_divergence_wins(self):
  with tempfile.TemporaryDirectory() as directory:
   bundle=Path(directory);(bundle/'runtime').mkdir()
   (bundle/'manifest.json').write_text(json.dumps({'run_id':'r'}))
   events=[{'schema_version':1,'run_id':'r','sequence':1,'kind':{'fields':[['all_ordinal','0'],['filtered_ordinal','']]}},
           {'schema_version':1,'run_id':'r','sequence':2,'kind':{'fields':[['all_ordinal','1'],['filtered_ordinal','0']]}}]
   (bundle/'runtime/events.jsonl').write_text(''.join(json.dumps(x)+'\n' for x in events))
   result=diag.correlate(bundle,{'filtered':{'equal':False,'record':1},'unfiltered':{'equal':False,'record':1}})
   self.assertEqual(result['divergence_context']['comparison'],'unfiltered')
 def test_replay_restores_effective_trace_environment(self):
  environment=os.environ.copy();environment['SLR_TRACE_TEST']='first'
  first=subprocess.run(self.argv(),env=environment,capture_output=True,text=True)
  self.assertEqual(first.returncode,0,first.stdout)
  bundle=next((self.repo/'runs').iterdir())
  environment['SLR_TRACE_TEST']='different';environment['SLR_TRACE_NEW']='unrequested'
  result=subprocess.run(self.argv()[:-2]+['--replay-from',str(bundle)],env=environment,capture_output=True,text=True)
  self.assertEqual(result.returncode,0,result.stdout)
  newest=max((self.repo/'runs').iterdir(),key=lambda p:p.name)
  traces=json.loads((newest/'manifest.json').read_text())['trace']['environment']
  self.assertEqual(traces['SLR_TRACE_TEST'],'first');self.assertNotIn('SLR_TRACE_NEW',traces)
 def test_partial_native_tail_retains_complete_records(self):
  path=self.repo/'partial.slrlog';data=(self.repo/'expected.slrlog').read_bytes()
  path.write_bytes(data+data[8:-1])
  prefix=diag.native_prefix(path)
  self.assertEqual(prefix['valid_records'],1);self.assertEqual(prefix['tail_error_offset'],len(data))
  self.assertFalse(prefix['acceptance_evidence'])
 def test_oracle_authority_reconstructs_observer_normalization(self):
  facts={'native_calls':[
   {'i':1,'class':'A','name':'f','descriptor':'(Ljava.lang.String;)Ljava.lang.String;','args':[{'kind':'I','payload':0x80000001}],'result':0x80000002},
   {'i':2,'class':'A','name':'g','descriptor':'(Ljava.lang.String;)V','args':[{'kind':'Ljava.lang.String;','payload':123}],'result':None}]}
  path=self.repo/'facts.json';path.write_text(json.dumps(facts))
  records=[('native','A','f',[('int',-2147483647)],('int',-2147483646),True),('native','A','g',[('int',1)],None,False)]
  self.assertEqual(diag.oracle_descriptors(path,records),[(['L'],'L'),(['L'],'V')])
 def test_oracle_authority_rejects_mismatched_row(self):
  path=self.repo/'facts.json';path.write_text(json.dumps({'native_calls':[{'i':1,'class':'A','name':'f','descriptor':'()I','args':[],'result':8}]}))
  with self.assertRaisesRegex(ValueError,'does not match'):diag.oracle_descriptors(path,[('native','A','f',[],('int',7),True)])
 def test_exact_per_ordinal_authority_preserves_primitive_overload(self):
  left=self.repo/'left.slrlog';right=self.repo/'right.slrlog'
  for path,word in [(left,7),(right,8)]:
   writer=diag.calllog.Writer(path);writer.record(diag.calllog.native_call('A','set',[word],void=True));writer.close()
  types={('A','set',1):[(['I'],'V'),(['L'],'V')]}
  self.assertFalse(diag.compare(left,right,types,authorities=[(['I'],'V')])['equal'])
  self.assertTrue(diag.compare(left,right,types,authorities=[(['L'],'V')])['equal'])
 def test_unfinished_wrapper_bundle_is_read_only_diagnostic(self):
  bundle=self.repo/'unfinished';runtime=bundle/'runtime';runtime.mkdir(parents=True)
  (bundle/'manifest.json').write_text(json.dumps({'run_id':'unfinished','status':'running'}))
  event={'schema_version':1,'run_id':'unfinished','sequence':1,'kind':{'event':'external','category':'state-checkpoint'}}
  (runtime/'events.jsonl').write_text(json.dumps(event)+'\n'+ '{partial')
  state={'schema_version':1,'sequence':1,'provenance':{'run_id':'unfinished'},'queues':{'ready':[3]}}
  (runtime/'states.jsonl').write_text(json.dumps(state)+'\n')
  data=(self.repo/'expected.slrlog').read_bytes();(runtime/'runtime.calllog').write_bytes(data+data[8:-1])
  before={str(path):path.read_bytes() for path in bundle.rglob('*') if path.is_file()}
  result=diag.analyze(bundle)
  self.assertEqual(result['status'],'unfinished');self.assertTrue(result['diagnostic_only']);self.assertFalse(result['acceptance'])
  self.assertEqual(result['nearest_preceding_state']['queues']['ready'],[3])
  self.assertEqual(result['native_prefixes']['filtered']['valid_records'],1)
  self.assertEqual(result['diagnostic_parse_errors'][0]['valid_records'],1)
  self.assertEqual(before,{str(path):path.read_bytes() for path in bundle.rglob('*') if path.is_file()})
  with self.assertRaisesRegex(ValueError,'running or unfinished'):diag.verify_artifacts(bundle,json.loads((bundle/'manifest.json').read_text()))
 def test_unique_bundles(self):
  result,first,_,_=self.invoke();self.assertEqual(result.returncode,0)
  result=subprocess.run(self.argv(),capture_output=True,text=True);self.assertEqual(result.returncode,0,result.stdout)
  self.assertEqual(len(list((self.repo/'runs').iterdir())),2)
 def test_successful_leader_cleans_descendants(self):
  result,_,manifest,_=self.invoke('spawn')
  self.assertEqual(result.returncode,0,result.stdout)
  time.sleep(2.1);self.assertFalse((self.repo/'escaped').exists())
 def test_primitive_reference_overload_fails_closed(self):
  record=('native','A','f',[('int',7)],None,False)
  types={('A','f',1):[(['I'],'V'),(['L'],'V')]}
  self.assertEqual(diag.masked(record,types)[3],[('int',7)])
  self.assertNotEqual(diag.masked(record,types)[3],[('int',8)])
 def test_strict_clock_results_remain_visible(self):
  first=('native','java.lang.System','currentTime',[],('float',1),True)
  second=('native','java.lang.System','currentTime',[],('float',2),True)
  self.assertNotEqual(diag.masked(first,{},False),diag.masked(second,{},False))
  self.assertEqual(diag.masked(first,{},True),diag.masked(second,{},True))
 def test_timeout_kills_descendants(self):
  result,_,manifest,_=self.invoke('sleep',.15)
  self.assertNotEqual(result.returncode,0);self.assertEqual(manifest['status'],'failed')
  time.sleep(2.1);self.assertFalse((self.repo/'escaped').exists())
 def test_cancellation_finalizes_bundle_and_kills_descendants(self):
  process=subprocess.Popen(self.argv('sleep'),stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
  deadline=time.monotonic()+5
  while time.monotonic()<deadline:
   if list((self.repo/'runs').glob('*/runtime')):break
   time.sleep(.02)
  process.send_signal(signal.SIGTERM);process.communicate(timeout=5)
  bundle=next((self.repo/'runs').iterdir())
  self.assertEqual(json.loads((bundle/'manifest.json').read_text())['status'],'interrupted')
  time.sleep(2.1);self.assertFalse((self.repo/'escaped').exists())
 def test_replay_from_uses_frozen_inputs_and_allows_binary_change(self):
  profile=self.repo/'profile.json';profile.write_text('{}')
  capture=self.repo/'capture';(capture/'observed').mkdir(parents=True)
  (capture/'clock-inputs.jsonl').write_text('{}\n')
  (capture/'clock-verification.json').write_text('{"fixture":1}')
  companion=self.repo/'timer-source';companion.mkdir()
  (companion/'frame-timer-inputs.jsonl').write_text('{}\n')
  (companion/'facts.json').write_text('{}')
  (capture/'frame-timer-manifest.json').write_text(json.dumps({'file':'../timer-source/frame-timer-inputs.jsonl','source_facts':'../timer-source/facts.json'}))
  (capture/'observed/original-facts.json').write_text('{}')
  subprocess.run(['git','-C',str(self.repo),'add','profile.json'],check=True)
  subprocess.run(['git','-C',str(self.repo),'-c','user.name=Test','-c','user.email=test@example.test','commit','-qm','profile'],check=True)
  argv=self.argv()[:-2]+['--profile',str(profile),'--replay',str(capture/'clock-inputs.jsonl'),'--frames','3','--','--mode=equal']
  first=subprocess.run(argv,capture_output=True,text=True);self.assertEqual(first.returncode,0,first.stdout)
  bundle=next((self.repo/'runs').iterdir())
  for path in ['expected.slrlog','all.slrlog','native.tsv','profile.json']:(self.repo/path).unlink()
  import shutil;shutil.rmtree(capture);shutil.rmtree(companion)
  with (self.repo/'fake').open('a') as stream:stream.write('\n# newer binary\n')
  result=subprocess.run(self.argv()[:-2]+['--replay-from',str(bundle)],capture_output=True,text=True)
  self.assertEqual(result.returncode,0,result.stdout+result.stderr)
  newest=max((self.repo/'runs').iterdir(),key=lambda p:p.name)
  manifest=json.loads((newest/'manifest.json').read_text())
  self.assertTrue(manifest['source_run']['binary_changed'])
  self.assertEqual(manifest['configuration']['frames'],3)
  self.assertEqual(manifest['comparison_policy']['masked_clock_results'],[])
 def test_analysis_rejects_missing_artifact_inventory(self):
  _,bundle,manifest,_=self.invoke();manifest['artifacts']={}
  (bundle/'manifest.json').write_text(json.dumps(manifest))
  with self.assertRaisesRegex(ValueError,'incomplete'):diag.analyze(bundle)

if __name__=='__main__':unittest.main()
