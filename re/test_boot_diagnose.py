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

HOOK=ROOT/'re/hook-run.py'
GIT=['git','-c','user.name=Test','-c','user.email=test@example.test']

def run_cli(*args,**kw):
 return subprocess.run([sys.executable,str(COMMAND),*map(str,args)],capture_output=True,text=True,**kw)

class RootAndRebuildTests(Fixture):
 def test_default_root_is_outside_the_worktree_and_env_overrides_it(self):
  with mock.patch.dict(os.environ,{'SLR_RUNS_ROOT':str(self.repo/'archive')}):
   self.assertEqual(diag.archive.default_runs_root('boot'),self.repo/'archive/boot/runs')
   self.assertEqual(diag.archive.default_runs_root('menu'),self.repo/'archive/menu/runs')
  with mock.patch.dict(os.environ,{'XDG_DATA_HOME':str(self.repo/'xdg')},clear=False):
   os.environ.pop('SLR_RUNS_ROOT',None)
   self.assertEqual(diag.archive.default_runs_root(),self.repo/'xdg/street-legal/boot/runs')
 def test_new_records_store_the_bundle_relative_to_the_runs_root(self):
  _,bundle,_,_=self.invoke()
  record=json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertEqual(record['bundle'],bundle.name)
  moved=self.repo/'moved';(self.repo/'runs').rename(moved)
  (self.repo/'runs-index.jsonl').rename(self.repo/'moved-index.jsonl')
  # the whole archive moved: the record still resolves under the new root
  self.assertEqual(diag.resolve_record(record,moved),(moved/bundle.name).resolve())
 def test_legacy_absolute_record_falls_back_to_run_id_under_the_root(self):
  _,bundle,_,_=self.invoke()
  path=self.repo/'runs-index.jsonl'
  record=json.loads(path.read_text().splitlines()[-1]);record['bundle']='/gone/worktree/re/out/boot/runs/'+bundle.name
  path.write_text(json.dumps(record)+'\n')
  rows=diag.read_index(self.repo/'runs')
  self.assertEqual(rows[0]['bundle_path'],str(bundle.resolve()))
  digest=json.loads(run_cli('digest',bundle.name,'--runs-root',self.repo/'runs').stdout)
  self.assertEqual(digest['run_id'],bundle.name)
 def test_safe_rebuild_backs_up_verifies_then_swaps_without_touching_bundles(self):
  self.invoke();self.invoke('mismatch')
  path=self.repo/'runs-index.jsonl'
  old=[json.loads(line) for line in path.read_text().splitlines()]
  for record in old: record['bundle']='/gone/'+record['run_id']
  path.write_text(''.join(json.dumps(record)+'\n' for record in old))
  before={str(p):diag.sha(p.read_bytes()) for p in (self.repo/'runs').rglob('*') if p.is_file()}
  original=path.read_bytes()
  result=json.loads(run_cli('index','--rebuild','--runs-root',self.repo/'runs').stdout)
  self.assertTrue(result['verified']);self.assertTrue(result['swapped'])
  self.assertEqual((result['previous_records'],result['indexed'],result['records']),(2,2,2))
  self.assertEqual(Path(result['backup']).read_bytes(),original,'the old index is kept byte for byte')
  new=[json.loads(line) for line in path.read_text().splitlines()]
  self.assertTrue(all(not Path(r['bundle']).is_absolute() for r in new))
  self.assertEqual(len(diag.read_index(self.repo/'runs')),2)
  self.assertTrue(all(r['bundle_path'] for r in diag.read_index(self.repo/'runs')))
  self.assertEqual(before,{str(p):diag.sha(p.read_bytes()) for p in (self.repo/'runs').rglob('*') if p.is_file()},'bundles are untouched')
  self.assertFalse(list(self.repo.glob('runs-index.jsonl.new-*')),'the candidate file is consumed by the swap')
 def test_rebuild_keeps_history_of_pruned_runs_and_dry_run_does_not_swap(self):
  self.invoke();self.invoke()
  path=self.repo/'runs-index.jsonl'
  lines=path.read_text().splitlines();gone=json.loads(lines[0]);gone['run_id']='pruned-run';gone['bundle']='/gone/pruned-run'
  path.write_text('\n'.join(lines+[json.dumps(gone)])+'\n')
  before=path.read_bytes()
  dry=json.loads(run_cli('index','--rebuild','--dry-run','--runs-root',self.repo/'runs').stdout)
  self.assertTrue(dry['verified']);self.assertFalse(dry['swapped']);self.assertEqual(path.read_bytes(),before)
  done=json.loads(run_cli('index','--rebuild','--runs-root',self.repo/'runs').stdout)
  self.assertEqual(done['carried_over'],1)
  rows={r['run_id']:r for r in diag.archive.read_lines(path)}
  self.assertTrue(rows['pruned-run']['bundle_missing'])
 def test_failed_verification_leaves_the_index_unchanged(self):
  self.invoke()
  path=self.repo/'runs-index.jsonl';before=path.read_bytes()
  with mock.patch.object(diag.archive,'resolve_bundle',return_value=None):
   with self.assertRaisesRegex(ValueError,'left unchanged'):diag.index_missing(self.repo/'runs',rebuild=True)
  self.assertEqual(path.read_bytes(),before)
 def test_unreadable_bundle_directory_is_reported_not_indexed(self):
  self.invoke();(self.repo/'runs/not-a-bundle').mkdir()
  result=json.loads(run_cli('index','--rebuild','--runs-root',self.repo/'runs').stdout)
  self.assertEqual(result['indexed'],1);self.assertEqual(result['skipped'][0]['bundle'],'not-a-bundle')
  self.assertTrue(result['verified'])

 def test_rebuild_can_adopt_a_legacy_archive_read_only_into_a_new_root(self):
  self.invoke();self.invoke('mismatch')
  legacy=self.repo/'runs'
  frozen={str(p):(diag.sha(p.read_bytes()),p.stat().st_mtime_ns) for p in self.repo.rglob('*') if p.is_file() and '.git/' not in str(p)}
  target=self.repo/'permanent/boot/runs'
  result=json.loads(run_cli('index','--rebuild','--source',legacy,'--runs-root',target).stdout)
  self.assertTrue(result['verified'] and result['swapped']);self.assertEqual((result['indexed'],result['carried_over']),(2,0))
  self.assertEqual(result['source'],str(legacy.resolve()));self.assertIsNone(result['backup'],'nothing existed to back up in the new root')
  after={str(p):(diag.sha(p.read_bytes()),p.stat().st_mtime_ns) for p in self.repo.rglob('*') if p.is_file() and '.git/' not in str(p) and 'permanent' not in str(p)}
  self.assertEqual(frozen,after,'the legacy archive, its index included, is byte- and mtime-identical')
  rows=diag.read_index(target);self.assertEqual(len(rows),2)
  self.assertTrue(all(Path(r['bundle']).is_absolute() and r['bundle_path'] for r in rows))

class ProvenanceTests(Fixture):
 def test_manifest_and_index_record_full_provenance(self):
  environment=os.environ.copy();environment['CLAUDE_SESSION_ID']='session-abc'
  subprocess.run(self.argv(),env=environment,capture_output=True,text=True,check=True)
  bundle=next((self.repo/'runs').iterdir())
  manifest=json.loads((bundle/'manifest.json').read_text());prov=manifest['provenance']
  head=diag.git_identity(self.repo)[0]['commit']
  self.assertEqual(prov['source_commit'],head)
  self.assertEqual(prov['session'],{'env':'CLAUDE_SESSION_ID','id':'session-abc'})
  self.assertEqual(Path(prov['worktree']).resolve(),self.repo.resolve())
  self.assertEqual(prov['source_diff_sha256'],manifest['source']['tracked_diff_sha256'])
  record=json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1])
  for key in ('source_commit','branch','worktree','session','source_dirty','source_diff_sha256'):self.assertIn(key,record)
  self.assertEqual(record['source_commit'],head);self.assertIsNotNone(record['branch'])
 def test_dirty_tree_is_flagged_with_the_diff_hash(self):
  (self.repo/'expected.slrlog').with_name('note.txt').write_text('untracked')
  (self.repo/'native.tsv').write_text('test.A\tf\t()I\n# edit\n')
  _,bundle,manifest,_=self.invoke()
  record=json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertTrue(record['source_dirty']);self.assertEqual(record['source_diff_sha256'],diag.sha((bundle/'source.diff').read_bytes()))
 def test_unresolvable_head_is_unknown_with_a_reason_never_null(self):
  with tempfile.TemporaryDirectory() as directory:
   value=diag.archive.provenance(directory)
  self.assertEqual(value['source_commit'],'unknown');self.assertIn('HEAD',value['source_commit_reason'])
 def test_backfill_only_from_reliable_evidence(self):
  archive=diag.archive
  self.assertEqual(archive.record_provenance({'source':{'commit':'abc','dirty':True}})['source_commit'],'abc')
  same={'source_run':{'source_changed':False,'source':{'commit':'def'}}}
  derived=archive.record_provenance(same);self.assertEqual((derived['source_commit'],derived['source_commit_basis']),('def','replay-source-unchanged'))
  changed=archive.record_provenance({'source_run':{'source_changed':True,'source':{'commit':'def'}}})
  self.assertEqual(changed['source_commit'],'unknown');self.assertIn('source.diff',changed['source_commit_reason'])
  self.assertEqual(archive.record_provenance({})['source_commit'],'unknown')

class ProgressTests(Fixture):
 def test_equal_run_matches_every_record(self):
  self.invoke()
  record=json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertEqual(record['matched_prefix'],{'filtered':1,'unfiltered':1});self.assertEqual(record['records_total'],{'filtered':1,'unfiltered':1})
  self.assertIsNone(record['first_mismatch_signature'])
 def test_mismatch_records_prefix_and_a_run_independent_signature(self):
  self.invoke('mismatch');shutil=__import__('shutil')
  first=json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1])
  shutil.rmtree(self.repo/'runs');(self.repo/'runs-index.jsonl').unlink()
  self.invoke('mismatch')
  second=json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertEqual(first['matched_prefix'],{'filtered':0,'unfiltered':0})
  self.assertNotEqual(first['run_id'],second['run_id'])
  self.assertTrue(first['first_mismatch_signature'])
  self.assertEqual(first['first_mismatch_signature'],second['first_mismatch_signature'])
  self.assertEqual(first['first_mismatch_values_signature'],second['first_mismatch_values_signature'])
 def test_signature_ignores_call_ids_but_not_the_native_identity(self):
  base={'comparisons':{'filtered':{'equal':False,'kind':'record_mismatch','record':5,'expected':['native','A','f',[],None,False],'actual':['native','A','g',[],None,False]}},
        'divergence_context':{'event':{'context':{'thread_name':'main','native':{'call_id':1,'class':'A','name':'f','signature':'()V'}}}}}
  other=json.loads(json.dumps(base));other['divergence_context']['event']['context']['native']['call_id']=999
  self.assertEqual(diag.archive.mismatch_signature(base),diag.archive.mismatch_signature(other))
  other['comparisons']['filtered']['expected'][2]='h'
  self.assertNotEqual(diag.archive.mismatch_signature(base)[0],diag.archive.mismatch_signature(other)[0])
 def test_length_mismatch_prefix_is_the_common_length(self):
  self.assertEqual(diag.archive.comparison_progress({'equal':False,'kind':'length_mismatch','record':4,'expected_records':9}),(3,9))
 def test_status_flags_a_regression_against_an_ancestor_commit(self):
  def commit(message):
   (self.repo/'f.txt').write_text(message);subprocess.run(['git','-C',str(self.repo),'add','.'],check=True)
   subprocess.run(['git','-C',str(self.repo),*GIT[1:],'commit','-qm',message],check=True)
   return subprocess.run(['git','-C',str(self.repo),'rev-parse','HEAD'],capture_output=True,text=True,check=True).stdout.strip()
  a=commit('a');b=commit('b')
  def row(run,commit_id,prefix,started):
   return {'run_id':run,'source_commit':commit_id,'started':started,'status':'failed','embedding':'slr-boot','configuration':{'frames':1},
           'matched_prefix':{'filtered':prefix,'unfiltered':prefix+5}}
  rows=[row('r1',a,908,'2026-01-01'),row('r2',b,700,'2026-01-02'),row('r3',b,6996,'2026-01-03')]
  report=diag.archive.trend_and_regressions(rows,self.repo)
  self.assertEqual([r['run_id'] for r in report['regressions']],['r2'])
  self.assertEqual(report['regressions'][0]['lost_records'],208)
  trend={t['commit']:t for t in report['trend']};self.assertEqual((trend[a]['best'],trend[b]['best'],trend[b]['latest']),(908,6996,6996))
  rows[1]['source_commit']='unknown'
  self.assertEqual(diag.archive.trend_and_regressions(rows,self.repo)['regressions'],[],'unknown commits are never compared')
  lines=diag.status_lines({**report,'unknown_commit_runs':0});self.assertTrue(lines[0].startswith('REGRESSION run r2'))
 def test_status_command_prints_trend_lines_and_check_regression_reports_the_prefix(self):
  _,bundle,_,_=self.invoke('mismatch')
  text=run_cli('status','--text','--runs-root',self.repo/'runs').stdout
  self.assertIn('best_prefix=0',text)
  binary=self.repo/'fake';binary.write_text(binary.read_text().replace("('mismatch','unstable')","('unstable',)"))
  result=subprocess.run(self.argv()[:-2]+['--check-regression',str(bundle)],capture_output=True,text=True)
  newest=max((self.repo/'runs').iterdir(),key=lambda p:p.name)
  check=json.loads((newest/'report.json').read_text())['regression_check']
  self.assertEqual((check['prior_matched_prefix'],check['matched_prefix'],check['regressed']),(0,1,False))

class FlakeCheckTests(Fixture):
 def test_signatures_decide_determinism(self):
  same=diag.flake_summary(['failed','failed'],['s','s'],['v','v']);self.assertFalse(same['nondeterministic']);self.assertTrue(same['signature_stable'])
  moved=diag.flake_summary(['failed','failed'],['s','t'],['v','w']);self.assertTrue(moved['nondeterministic']);self.assertEqual(moved['distinct_signatures'],2)
  values=diag.flake_summary(['failed','failed'],['s','s'],['v','w']);self.assertTrue(values['nondeterministic']);self.assertTrue(values['signature_stable']);self.assertFalse(values['values_stable'])
 def test_flake_check_runs_n_times_and_exits_3_when_runs_disagree(self):
  binary=self.repo/'fake'
  source=binary.read_text().replace("mode=next((x.split('=',1)[1] for x in args if x.startswith('--mode=')),'equal')",
   "mode=next((x.split('=',1)[1] for x in args if x.startswith('--mode=')),'equal')\ncounter=out.parent.parent/'n'\nn=int(counter.read_text())+1 if counter.exists() else 1\ncounter.write_text(str(n))\nif n==2: mode='mismatch'")
  binary.write_text(source);binary.chmod(0o755)
  subprocess.run(['git','-C',str(self.repo),'add','fake'],check=True);subprocess.run(['git','-C',str(self.repo),*GIT[1:],'commit','-qm','flaky'],check=True)
  result=subprocess.run(self.argv()[:-2]+['--flake-check','3'],capture_output=True,text=True)
  self.assertEqual(result.returncode,3,result.stdout+result.stderr)
  records=[json.loads(l) for l in (self.repo/'runs-index.jsonl').read_text().splitlines()]
  self.assertEqual(len(records),3);self.assertTrue(records[0]['flake']['nondeterministic']);self.assertEqual(len(records[0]['flake']['first_mismatch_signatures']),3)
 def test_stable_failure_is_deterministic_and_keeps_the_runs_own_exit_status(self):
  result=subprocess.run(self.argv('mismatch')[:-2]+['--flake-check','2','--','--mode=mismatch'],capture_output=True,text=True)
  self.assertNotEqual(result.returncode,3)
  records=[json.loads(l) for l in (self.repo/'runs-index.jsonl').read_text().splitlines()]
  self.assertFalse(records[0]['flake']['nondeterministic']);self.assertTrue(records[0]['flake']['signature_stable'])

class DigestTests(Fixture):
 def digest(self,target,*extra):
  result=run_cli('digest',target,'--json','--runs-root',self.repo/'runs',*extra);self.assertEqual(result.returncode,0,result.stderr)
  return json.loads(result.stdout)
 def test_digest_of_a_failed_run_names_mismatch_chain_and_commands(self):
  _,bundle,_,_=self.invoke('mismatch')
  value=self.digest(bundle.name)
  self.assertEqual(value['status'],'failed');self.assertEqual(value['first_mismatch']['record'],1)
  self.assertEqual(value['native_call']['call_id'],55)
  self.assertTrue(value['causal_chain'] and value['causal_chain'][-1]['sequence']==10)
  self.assertEqual(value['provenance']['source_commit'],diag.git_identity(self.repo)[0]['commit'])
  self.assertEqual(value['progress']['matched_prefix'],{'filtered':0,'unfiltered':0})
  self.assertIn('--replay-from '+str(bundle.resolve()),value['commands']['replay_from']);self.assertIn('--check-regression',value['commands']['check_regression'])
  self.assertLessEqual(value['bytes'],diag.DIGEST_BYTES);self.assertIn('failure',value);self.assertFalse(value['failure']['present'])
  self.assertEqual(value['limits']['chain_events'],20)
 def test_digest_accepts_a_path_and_a_unique_prefix_and_rejects_ambiguity(self):
  self.invoke();self.invoke()
  names=sorted(p.name for p in (self.repo/'runs').iterdir())
  self.assertEqual(self.digest(str(self.repo/'runs'/names[0]))['run_id'],names[0])
  self.assertNotEqual(run_cli('digest',names[0][:4],'--runs-root',self.repo/'runs').returncode,0)
 def test_digest_of_an_unfinished_bundle_works_and_never_executes_the_game(self):
  bundle=self.repo/'runs/unfinished';runtime=bundle/'runtime';runtime.mkdir(parents=True)
  (bundle/'manifest.json').write_text(json.dumps({'run_id':'unfinished','status':'running'}))
  rows=[{'schema_version':1,'run_id':'unfinished','sequence':n,'kind':{'event':'external','category':'tick','fields':[['n',str(n)]]}} for n in range(1,60)]
  (runtime/'events.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows)+'{partial')
  data=(self.repo/'expected.slrlog').read_bytes();(runtime/'runtime.calllog').write_bytes(data+data[8:-1])
  (self.repo/'fake').unlink()  # nothing may try to run the binary
  before={str(p):p.read_bytes() for p in bundle.rglob('*') if p.is_file()}
  value=self.digest('unfinished')
  self.assertEqual(value['status'],'running');self.assertEqual(len(value['causal_chain']),20)
  self.assertEqual(value['causal_chain'][-1]['sequence'],59,'the torn final line is dropped, not guessed');self.assertTrue(value['truncated']['causal_chain'])
  self.assertEqual(value['native_prefixes']['filtered']['valid_records'],1)
  self.assertEqual(before,{str(p):p.read_bytes() for p in bundle.rglob('*') if p.is_file()})
 def test_digest_reports_ring_limits_and_stays_bounded(self):
  bundle=self.repo/'runs/crashed';runtime=bundle/'runtime';runtime.mkdir(parents=True)
  (bundle/'manifest.json').write_text(json.dumps({'run_id':'crashed','status':'failed','error':'x'*5000}))
  ring=[{'sequence':n,'kind':{'event':'external','category':'c','fields':[['blob','y'*5000]]},'context':{}} for n in range(200)]
  (runtime/'failure.json').write_text(json.dumps({'message':'boom','location':'a.rs:1:1','backtrace':'bt\n'*5000,'recent_events':ring,
    'diagnostics':{'capacity':64,'events_total':5000,'ring_retained':64,'ring_evicted':4936}}))
  value=self.digest('crashed','--max-bytes','8192')
  self.assertLessEqual(value['bytes'],8192);self.assertTrue(value['failure']['ring']['ring_truncated']);self.assertEqual(value['failure']['ring']['evicted'],4936)
  self.assertTrue(value['truncated']['failure_ring']);self.assertLess(len(value['error']),700);self.assertEqual(value['failure']['message'],'boom')

class FixtureTests(Fixture):
 def setUp(self):
  super().setUp();r=self.repo
  (r/'re/oracle').mkdir(parents=True);(r/'re/out/boot').mkdir(parents=True);(r/'re/out/snapshots').mkdir()
  (r/'re/oracle/probe.py').write_text('print(1)\n');(r/'re/out/snapshots/snap.pages').write_bytes(b'v1')
  (r/'re/out/boot/a.json').write_text(json.dumps({'x':1}))
  (r/'re/out/boot/b.json').write_text(json.dumps({'producer_sources':{'probe.py':diag.fixtures.sha256_file(r/'re/oracle/probe.py')}}))
  (r/'re/fixtures-manifest.json').write_text(json.dumps({'schema_version':1,'producer_dirs':['re/oracle'],'fixtures':[
   {'path':'re/out/boot/a.json','producer':'re/oracle/probe.py','regenerate':'python3 re/oracle/probe.py a','inputs':['re/out/snapshots/snap.*']},
   {'path':'re/out/boot/b.json','producer':'re/oracle/probe.py','regenerate':'python3 re/oracle/probe.py b'},
   {'path':'re/out/boot/c.json','producer':'re/oracle/probe.py','regenerate':'python3 re/oracle/probe.py c'}]}))
  self.lock=r/'re/out/lock.json'
 def status(self,*extra):
  return run_cli('fixtures','status','--repository',self.repo,'--lock',self.lock,*extra)
 def test_missing_unlocked_ok_and_one_line_each(self):
  result=self.status();lines=result.stdout.strip().splitlines()
  self.assertEqual(result.returncode,0)
  self.assertTrue(any(l.startswith('MISSING') and 'c.json' in l and 'python3 re/oracle/probe.py c' in l for l in lines))
  self.assertTrue(any(l.startswith('UNLOCKED') and 'a.json' in l for l in lines));self.assertFalse(any(l.startswith(('STALE','CHANGED')) for l in lines))
  self.assertTrue(lines[-1].startswith('fixtures:'))
 def test_input_regenerated_after_the_fixture_is_stale(self):
  self.assertEqual(run_cli('fixtures','lock','--repository',self.repo,'--lock',self.lock).returncode,0)
  self.assertEqual(self.status().returncode,0)
  (self.repo/'re/out/snapshots/snap.pages').write_bytes(b'v2')
  result=self.status();self.assertEqual(result.returncode,1)
  self.assertIn('STALE',result.stdout);self.assertIn('snap.pages was regenerated after this fixture',result.stdout)
 def test_changed_fixture_producer_edit_and_embedded_producer_hash_are_detected(self):
  run_cli('fixtures','lock','--repository',self.repo,'--lock',self.lock)
  (self.repo/'re/out/boot/a.json').write_text(json.dumps({'x':2}))
  self.assertIn('CHANGED',self.status().stdout)
  (self.repo/'re/out/boot/a.json').write_text(json.dumps({'x':1}))
  (self.repo/'re/oracle/probe.py').write_text('print(2)\n')
  out=self.status().stdout
  self.assertIn('STALE   ',out.replace('STALE','STALE  ',1)) if False else None
  self.assertIn('producer probe.py changed since generation',out);self.assertIn('producer re/oracle/probe.py changed since generation',out)

class HookRunTests(FixtureTests):
 def hook(self,*steps,runs=None):
  argv=[sys.executable,str(HOOK),'--repository',str(self.repo),'--runs-root',str(runs or self.repo/'hookruns'),'--lock',str(self.lock)]
  for step in steps:argv+=['--step',step]
  return subprocess.run(argv,capture_output=True,text=True)
 def records(self):return [json.loads(l) for l in (self.repo/'hookruns-index.jsonl').read_text().splitlines()] if (self.repo/'hookruns-index.jsonl').exists() else [json.loads(l) for l in (self.repo/'runs-index.jsonl').read_text().splitlines()]
 def test_green_run_is_recorded_with_output_exit_status_commit_and_duration(self):
  (self.repo/'ok.py').write_text('print("hello from step")\n')
  result=self.hook('one=python3 ok.py','two=python3 ok.py')
  self.assertEqual(result.returncode,0,result.stderr);self.assertIn('hello from step',result.stdout)
  record=json.loads((self.repo/'index'/'x').read_text()) if False else json.loads((self.repo/'runs-index.jsonl').read_text().splitlines()[-1]) if False else None
  index=self.repo/'runs-index.jsonl' if False else (self.repo/'hookruns').resolve().parent/'runs-index.jsonl'
  record=json.loads(index.read_text().splitlines()[-1])
  self.assertEqual((record['kind'],record['status']),('hook','completed'));self.assertEqual(record['source_commit'],diag.git_identity(self.repo)[0]['commit'])
  self.assertEqual([s['name'] for s in record['steps']],['one','two']);self.assertTrue(all(s['exit_code']==0 for s in record['steps']));self.assertGreaterEqual(record['duration_seconds'],0)
  bundle=(self.repo/'hookruns'/record['run_id']);self.assertIn('hello from step',(bundle/'one.txt').read_text())
  manifest=json.loads((bundle/'manifest.json').read_text());diag.verify_artifacts(bundle,manifest)
 def test_failing_step_fails_the_hook_with_its_own_status_and_stops(self):
  (self.repo/'bad.py').write_text('import sys;print("broken");sys.exit(7)\n');(self.repo/'ok.py').write_text('print(1)\n')
  result=self.hook('fmt=python3 bad.py','test=python3 ok.py')
  self.assertEqual(result.returncode,7)
  record=json.loads(((self.repo/'hookruns').resolve().parent/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertEqual(record['status'],'failed');self.assertEqual(len(record['steps']),1);self.assertIn('fmt exited 7',record['error'])
  self.assertIn('broken',json.loads((self.repo/'hookruns'/record['run_id']/'report.json').read_text())['step_tail'])
 def test_stale_fixture_stops_before_any_step_runs(self):
  run_cli('fixtures','lock','--repository',self.repo,'--lock',self.lock)
  (self.repo/'re/out/snapshots/snap.pages').write_bytes(b'regenerated')
  (self.repo/'marker.py').write_text('open("ran","w").write("x")\n')
  result=self.hook('t=python3 marker.py')
  self.assertEqual(result.returncode,3);self.assertFalse((self.repo/'ran').exists())
  self.assertIn('regenerated after this fixture',result.stderr);self.assertIn('python3 re/oracle/probe.py a',result.stderr)
  record=json.loads(((self.repo/'hookruns').resolve().parent/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertEqual(record['status'],'failed');self.assertEqual(record['steps'],[])
 def test_fixture_changed_during_the_run_fails_even_when_the_steps_pass(self):
  (self.repo/'touch.py').write_text('import pathlib;pathlib.Path("re/out/snapshots/snap.pages").write_bytes(b"mid-run")\n')
  result=self.hook('t=python3 touch.py')
  self.assertEqual(result.returncode,4,result.stderr);self.assertIn('snap.pages: modified during the run',result.stderr)
  record=json.loads(((self.repo/'hookruns').resolve().parent/'runs-index.jsonl').read_text().splitlines()[-1])
  self.assertEqual(record['status'],'failed');self.assertIn('fixtures changed during the run',record['error'])
 def test_recording_failure_never_changes_the_verdict(self):
  (self.repo/'ok.py').write_text('print(1)\n');blocker=self.repo/'blocker';blocker.write_text('file, not a directory')
  result=self.hook('t=python3 ok.py',runs=blocker/'runs');self.assertEqual(result.returncode,0,result.stderr)
 def test_hook_uses_the_same_three_cargo_commands_as_before(self):
  spec=importlib.util.spec_from_file_location('hook',HOOK);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
  self.assertEqual([c for _,c in module.STEPS],[['cargo','fmt','--all','--','--check'],['cargo','clippy','--workspace','--all-targets','--','-D','warnings'],['cargo','test','--workspace']])
  text=(ROOT/'.githooks/pre-commit').read_text()
  for command in ('cargo fmt --all -- --check','cargo clippy --workspace --all-targets -- -D warnings','cargo test --workspace'):self.assertIn(command,text)

class MenuRootTests(unittest.TestCase):
 def test_window_sessions_register_in_the_archive_index(self):
  spec=importlib.util.spec_from_file_location('menu_for_root',ROOT/'re/menu-diagnose.py');menu=importlib.util.module_from_spec(spec);spec.loader.exec_module(menu)
  with mock.patch.dict(os.environ,{'SLR_RUNS_ROOT':'/archive'}):
   self.assertEqual(diag.archive.default_runs_root('menu'),Path('/archive/menu/runs'))
  self.assertEqual(menu.DEFAULT_ROOT,diag.archive.default_runs_root('menu'))


if __name__=='__main__':unittest.main()
