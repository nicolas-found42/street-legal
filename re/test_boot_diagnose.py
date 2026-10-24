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
from unittest import mock

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
if '--wall-clock' in args: event['wall_ns']=123456
if mode=='event-id': event['run_id']='wrong'
if '--clock-replay' in args:
 tape=pathlib.Path(args[args.index('--clock-replay')+1]);assert json.loads((tape.parent/'clock-verification.json').read_text())['fixture']==1;assert (tape.parent/'observed/original-facts.json').exists()
(out/'events.jsonl').write_text('broken' if mode=='bad-events' else json.dumps({'schema_version':1,'run_id':rid,'sequence':9,'kind':{'event':'external','category':'state-checkpoint'}})+'\\n'+json.dumps(event)+'\\n'+json.dumps({'schema_version':1,'run_id':rid,'sequence':11,'kind':{'event':'external','category':'capture-end'}})+'\\n')
if '--snapshot-native' in args: (out/'divergence.json').write_text(json.dumps({'schema_version':1,'run_id':rid,'native_call_id':55}))
(out/'states.jsonl').write_text(json.dumps({'schema_version':1,'sequence':9,'provenance':{'run_id':rid},'queues':{'ready':[1]}})+'\\n')
if mode=='empty-states': (out/'states.jsonl').write_text('')
if mode=='missing-events': (out/'events.jsonl').unlink()
'''

class ProcessGroupTests(unittest.TestCase):
 def test_macos_exited_group_permission_error_is_harmless(self):
  for listing in ('', '123 Z\n123 Z+\n999 S\n'):
   with self.subTest(listing=listing):
    with mock.patch.object(diag.sys,'platform','darwin'), mock.patch.object(diag.os,'killpg',side_effect=PermissionError(1,'denied')), mock.patch.object(diag.subprocess,'run',return_value=subprocess.CompletedProcess([],0,listing,'')):
     diag.signal_process_group(123,signal.SIGKILL)
 def test_live_group_permission_error_remains_fatal(self):
  with mock.patch.object(diag.sys,'platform','darwin'), mock.patch.object(diag.os,'killpg',side_effect=PermissionError(1,'denied')), mock.patch.object(diag.subprocess,'run',return_value=subprocess.CompletedProcess([],0,'123 Z\n123 S\n','')):
   with self.assertRaises(PermissionError):diag.signal_process_group(123,signal.SIGKILL)
 def test_process_inspection_failure_remains_fatal(self):
  with mock.patch.object(diag.sys,'platform','darwin'), mock.patch.object(diag.os,'killpg',side_effect=PermissionError(1,'denied')), mock.patch.object(diag.subprocess,'run',side_effect=subprocess.CalledProcessError(1,'ps')):
   with self.assertRaises(subprocess.CalledProcessError):diag.signal_process_group(123,signal.SIGKILL)
 def test_non_macos_permission_error_remains_fatal(self):
  with mock.patch.object(diag.sys,'platform','linux'), mock.patch.object(diag.os,'killpg',side_effect=PermissionError(1,'denied')):
   with self.assertRaises(PermissionError):diag.signal_process_group(123,signal.SIGKILL)

class Fixture(unittest.TestCase):
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
class RunTests(Fixture):
 def test_equal_complete_and_immutable_analysis(self):
  result,bundle,manifest,report=self.invoke()
  self.assertEqual(result.returncode,0,result.stderr+result.stdout)
  self.assertEqual(manifest['status'],'completed');self.assertTrue(report['comparisons']['unfiltered']['equal'])
  self.assertEqual(diag.analyze(bundle)['status'],'completed')
  (bundle/'runtime/runtime.calllog').write_bytes(b'changed')
  with self.assertRaisesRegex(ValueError,'artifact changed'):diag.analyze(bundle)
 def test_bundle_retains_untracked_source_bytes_and_mode(self):
  source=self.repo/'src/new tool.py';source.parent.mkdir()
  original=b'#!/usr/bin/env python3\nprint("untracked implementation")\n'
  source.write_bytes(original);source.chmod(0o755)
  before,_=diag.git_identity(self.repo)
  result,bundle,manifest,_=self.invoke()
  self.assertEqual(result.returncode,0,result.stdout+result.stderr)
  self.assertEqual(manifest['source'],before)
  archived=manifest['source_untracked'][0]
  self.assertEqual(archived['path'],'src/new tool.py')
  self.assertEqual(archived['mode'],0o755)
  saved=bundle/archived['snapshot']
  source.unlink()
  self.assertEqual(saved.read_bytes(),original)
  self.assertEqual(diag.analyze(bundle)['status'],'completed')
  saved.write_bytes(b'changed source')
  with self.assertRaisesRegex(ValueError,'artifact changed'):diag.analyze(bundle)
 def test_nested_manifest_source_is_inventoried(self):
  source=self.repo/'src/manifest.json';source.parent.mkdir()
  source.write_bytes(b'{"source":true}')
  result,bundle,manifest,_=self.invoke()
  self.assertEqual(result.returncode,0,result.stdout+result.stderr)
  snapshot='source-untracked/src/manifest.json'
  self.assertIn(snapshot,manifest['artifacts'])
  self.assertEqual(diag.analyze(bundle)['status'],'completed')
  (bundle/snapshot).write_bytes(b'{"source":false}')
  with self.assertRaisesRegex(ValueError,'artifact changed'):diag.analyze(bundle)
 def test_untracked_source_change_during_archive_is_rejected(self):
  source=self.repo/'new.py';source.write_bytes(b'original')
  identity,_=diag.git_identity(self.repo)
  bundle=self.repo/'runs';bundle.mkdir()
  copy=diag.shutil.copyfileobj
  def changing_copy(original,saved):
   copy(original,saved)
   source.write_bytes(b'modified during capture')
  with mock.patch.object(diag.shutil,'copyfileobj',side_effect=changing_copy):
   with self.assertRaisesRegex(ValueError,'changed while archiving'):
    diag.archive_untracked_sources(self.repo,bundle,identity)
 def test_untracked_symlink_fails_closed(self):
  (self.repo/'new.py').symlink_to(self.repo/'fake')
  result,_,manifest,report=self.invoke()
  self.assertNotEqual(result.returncode,0)
  self.assertEqual(manifest['status'],'failed')
  self.assertIn('symlink requires explicit provenance',report['error'])
 def test_untracked_archive_mode_change_is_rejected(self):
  (self.repo/'new.py').write_bytes(b'original')
  _,bundle,manifest,_=self.invoke()
  saved=bundle/manifest['source_untracked'][0]['snapshot']
  saved.chmod(0o755)
  with self.assertRaisesRegex(ValueError,'archive mode changed'):diag.analyze(bundle)
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

class ArchiveTests(Fixture):
 def test_index_records_every_bundle_and_queries(self):
  self.invoke()
  first=next(iter((self.repo/'runs').iterdir()))
  self.invoke()
  index=json.loads(subprocess.run([sys.executable,str(COMMAND),'list','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(len(index),2)
  self.assertEqual(index[0]['status'],'completed')
  self.assertEqual(index[0]['source_commit'],diag.git_identity(self.repo)[0]['commit'])
  failed=subprocess.run([sys.executable,str(COMMAND),'query','--runs-root',str(self.repo/'runs'),'--status','failed'],capture_output=True,text=True,check=True)
  self.assertEqual(json.loads(failed.stdout),[])
  latest=json.loads(subprocess.run([sys.executable,str(COMMAND),'status','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(latest['records'],2);self.assertIsNone(latest['latest_failed'])
  self.assertTrue((self.repo/'runs-index.jsonl').exists())
  self.assertEqual(len(list((self.repo/'runs').iterdir())),2)
 def test_index_backfills_a_bundle_written_before_the_index_existed(self):
  _,bundle,_,_=self.invoke()
  (self.repo/'runs-index.jsonl').unlink()
  added=json.loads(subprocess.run([sys.executable,str(COMMAND),'index','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(added['added'],1);self.assertEqual(added['skipped'],[])
  self.assertEqual(added['records'][0]['run_id'],bundle.name)
  again=json.loads(subprocess.run([sys.executable,str(COMMAND),'index','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(again['added'],0)
 def test_index_reconstructs_a_bundle_whose_wrapper_was_killed(self):
  _,bundle,_,_=self.invoke('mismatch')
  # A wrapper killed before it could write its files leaves only the
  # crash-surviving markers behind.
  (bundle/'manifest.json').unlink();(bundle/'report.json').unlink()
  (self.repo/'runs-index.jsonl').unlink()
  added=json.loads(subprocess.run([sys.executable,str(COMMAND),'index','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(added['skipped'],[])
  record=added['records'][0]
  self.assertEqual(record['run_id'],bundle.name)
  self.assertEqual(record['status'],'completed')
  self.assertIn('capture markers',record.get('reconstructed',''))
  self.assertIsNone(record['comparisons'].get('filtered',{}).get('equal'))
 def test_reconstructed_bundle_gets_a_labelled_derived_start(self):
  _,bundle,_,_=self.invoke()
  (bundle/'manifest.json').unlink();(bundle/'report.json').unlink()
  (self.repo/'runs-index.jsonl').unlink()
  added=json.loads(subprocess.run([sys.executable,str(COMMAND),'index','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  record=added['records'][0]
  # A killed wrapper records no clock. Without a fallback the bundle would be
  # invisible to `--since` and to the ordering in `status`.
  self.assertTrue(record['started'])
  self.assertTrue(record['started_derived'])
  since=json.loads(subprocess.run([sys.executable,str(COMMAND),'query','--runs-root',str(self.repo/'runs'),'--since','2000-01-01'],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(len(since),1)
  recorded=json.loads(subprocess.run([sys.executable,str(COMMAND),'query','--runs-root',str(self.repo/'runs'),'--since','2999-01-01'],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(recorded,[])
 def test_index_orders_by_start_time_not_append_order(self):
  self.invoke()
  self.invoke()
  # `invoke` returns an arbitrary bundle; the newest is the one to expect.
  second=max((self.repo/'runs').iterdir(),
             key=lambda path:json.loads((path/'manifest.json').read_text())['started'])
  path=self.repo/'runs-index.jsonl'
  lines=path.read_text().splitlines()
  path.write_text('\n'.join(reversed(lines))+'\n')
  latest=json.loads(subprocess.run([sys.executable,str(COMMAND),'status','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(latest['latest']['run_id'],second.name)
 def test_index_records_the_first_mismatch_and_native_call(self):
  _,_,_,report=self.invoke('mismatch')
  record=json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertEqual(record['status'],'failed')
  self.assertEqual(record['first_mismatch']['comparison'],'filtered')
  self.assertEqual(record['first_mismatch']['record'],1)
  self.assertEqual(record['native_call_id'],55)
  self.assertTrue(record['targeted_rerun']['snapshot_valid'])
 def test_status_calls_out_an_outstanding_failure(self):
  _,bundle,_,_=self.invoke('mismatch')
  latest=json.loads(subprocess.run([sys.executable,str(COMMAND),'status','--runs-root',str(self.repo/'runs')],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(latest['latest_failed']['run_id'],bundle.name)
  self.assertEqual(len(latest['outstanding']),1)
 def test_identical_inputs_share_content_store_bytes(self):
  self.invoke()
  self.invoke()
  bundles=sorted((self.repo/'runs').iterdir())
  first,second=[next(bundle.glob('inputs/native_api-*')) for bundle in bundles]
  self.assertEqual(first.read_bytes(),second.read_bytes())
  # Each bundle owns independent bytes (a hash must never be shared through a
  # hard link); on a reflink filesystem the store's blocks are still shared.
  self.assertNotEqual(first.stat().st_ino,second.stat().st_ino,
                      "bundles must stay independently verifiable")
  stored=self.repo/'inputs-store'
  unique=set()
  for bundle in bundles:
   manifest=json.loads((bundle/'manifest.json').read_text())
   unique.update(record['sha256'] for record in manifest['inputs'].values())
   unique.add(manifest['binary_snapshot']['sha256'])
  self.assertEqual({entry.name for entry in stored.iterdir()},unique,
                   "the store holds exactly the distinct input contents")
  self.assertEqual(diag.analyze(bundles[0])['status'],'completed')
  self.assertEqual(diag.analyze(bundles[1])['status'],'completed')
  (bundles[0]/'inputs/native_api-native.tsv').write_bytes(b'changed')
  with self.assertRaisesRegex(ValueError,'artifact changed'):diag.analyze(bundles[0])
  self.assertEqual(diag.analyze(bundles[1])['status'],'completed',"a hard link is a real file, per-bundle verification still applies")
 def test_retention_keeps_newest_and_spares_running(self):
  for _ in range(4):self.invoke()
  bundles=sorted((self.repo/'runs').iterdir())
  running=bundles[0];manifest=json.loads((running/'manifest.json').read_text())
  manifest['status']='running';(running/'manifest.json').write_text(json.dumps(manifest))
  result=subprocess.run([sys.executable,str(COMMAND),'prune','--runs-root',str(self.repo/'runs'),'--keep','2'],capture_output=True,text=True,check=True)
  pruned=json.loads(result.stdout)
  self.assertEqual([row['name'] for row in pruned['removed']],[bundles[1].name])
  self.assertEqual(pruned['retained_running'],[str(running.resolve())])
  self.assertEqual(sorted(path.name for path in (self.repo/'runs').iterdir()),
                   sorted([running.name,bundles[-2].name,bundles[-1].name]),
                   "a running bundle is never removed, even beyond the keep count")
  index=[json.loads(line) for line in (self.repo/'runs-index.jsonl').read_text().splitlines()]
  self.assertEqual(len(index),4,"history survives pruning")
 def test_prune_dry_run_removes_nothing_and_drops_orphaned_store_entries(self):
  self.invoke();self.invoke()
  dry=json.loads(subprocess.run([sys.executable,str(COMMAND),'prune','--runs-root',str(self.repo/'runs'),'--keep','1','--dry-run'],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(len(dry['removed']),1);self.assertEqual(len(list((self.repo/'runs').iterdir())),2)
  pruned=json.loads(subprocess.run([sys.executable,str(COMMAND),'prune','--runs-root',str(self.repo/'runs'),'--keep','1'],capture_output=True,text=True,check=True).stdout)
  self.assertEqual(len(pruned['removed']),1)
  kept=next(iter((self.repo/'runs').iterdir()))
  self.assertEqual(diag.analyze(kept)['status'],'completed',"the retained bundle still verifies")
  kept_paths={path.resolve() for path in (self.repo/'runs').rglob('*') if path.is_file()}
  for entry in (self.repo/'inputs-store').iterdir():
   self.assertIn(entry.stat().st_ino,{path.stat().st_ino for path in kept_paths},
                 "every surviving store entry is linked from a live bundle")
 def test_keep_prunes_after_the_run(self):
  self.invoke();self.invoke()
  result=subprocess.run(self.argv()[:-2]+['--keep','1'],capture_output=True,text=True)
  self.assertEqual(result.returncode,0,result.stdout)
  self.assertEqual(len(list((self.repo/'runs').iterdir())),1)
 def test_repeat_records_flake_rate(self):
  argv=self.argv()
  result=subprocess.run(argv[:-2]+['--repeat','3'],capture_output=True,text=True)
  self.assertEqual(result.returncode,0,result.stdout)
  records=[json.loads(line) for line in (self.repo/'runs-index.jsonl').read_text().splitlines()]
  self.assertEqual(len(records),3)
  self.assertEqual([record['flake']['iteration'] for record in records],[1,2,3])
  self.assertEqual(records[0]['flake']['iterations'],3)
  self.assertEqual(records[0]['flake']['failed'],0)
  self.assertFalse(records[0]['flake']['nondeterministic'])
  self.assertEqual(len(list((self.repo/'runs').iterdir())),3)
 def test_repeat_surfaces_an_intermittent_failure(self):
  binary=self.repo/'fake'
  source=binary.read_text().replace("mode=next((x.split('=',1)[1] for x in args if x.startswith('--mode=')),'equal')",
   "mode=next((x.split('=',1)[1] for x in args if x.startswith('--mode=')),'equal')\nimport pathlib as _p\ncounter=out.parent.parent/'flaky-counter'\nn=int(counter.read_text())+1 if counter.exists() else 1\ncounter.write_text(str(n))\nif n==2: mode='mismatch'")
  binary.write_text(source);binary.chmod(0o755)
  subprocess.run(['git','-C',str(self.repo),'add','fake'],check=True)
  subprocess.run(['git','-C',str(self.repo),'-c','user.name=Test','-c','user.email=test@example.test','commit','-qm','flaky'],check=True)
  result=subprocess.run(self.argv()[:-2]+['--repeat','3'],capture_output=True,text=True)
  self.assertNotEqual(result.returncode,0)
  records=[json.loads(line) for line in (self.repo/'runs-index.jsonl').read_text().splitlines()]
  self.assertEqual(records[0]['flake']['failed'],1)
  self.assertTrue(records[0]['flake']['nondeterministic'])
  self.assertEqual(records[0]['flake']['statuses'].count('failed'),1)
 def test_issue_dry_run_names_reproduction_commands(self):
  argv=self.argv('mismatch')
  marker=argv.index('--')
  result=subprocess.run(argv[:marker]+['--file-issue','dry-run']+argv[marker:],capture_output=True,text=True)
  self.assertNotEqual(result.returncode,0)
  bundle=max((self.repo/'runs').iterdir(),key=lambda path:path.name)
  report=json.loads((bundle/'report.json').read_text())
  command=report['issue_command']
  self.assertEqual(command[:3],['gh','issue','create'])
  self.assertIn('--replay-from',command[6])
  self.assertIn(str(bundle),command[6])
  self.assertIn('--check-regression',command[6])
 def test_wall_clock_is_recorded_and_ignored_by_comparison(self):
  result=subprocess.run(self.argv()[:-2]+['--wall-clock'],capture_output=True,text=True)
  self.assertEqual(result.returncode,0,result.stdout)
  bundle=next(iter((self.repo/'runs').iterdir()))
  manifest=json.loads((bundle/'manifest.json').read_text())
  self.assertTrue(manifest['configuration']['wall_clock'])
  self.assertIn('--wall-clock',manifest['child_arguments'])
  self.assertTrue(manifest['environment']['rustc'].startswith('rustc '))
  self.assertIn('uname',manifest['environment'])
  events=[json.loads(line) for line in (bundle/'runtime/events.jsonl').read_text().splitlines()]
  native=[event for event in events if event['kind'].get('category')=='native-call']
  self.assertTrue(native and all(isinstance(event['wall_ns'],int) for event in native))
  stamps=[event['wall_ns'] for event in native]
  self.assertEqual(sorted(stamps),stamps,"timestamps are monotonic")
 def test_environment_is_captured_even_without_wall_clock(self):
  self.invoke()
  manifest=json.loads((next(iter((self.repo/'runs').iterdir()))/'manifest.json').read_text())
  self.assertEqual(manifest['environment']['schema_version'],1)
  self.assertIsNone(manifest['environment']['wine'])
  events=(next(iter((self.repo/'runs').iterdir()))/'runtime/events.jsonl').read_text().splitlines()
  self.assertTrue(events and all('wall_ns' not in line for line in events))

if __name__=='__main__':unittest.main()
