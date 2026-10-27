from tests.support import *
from unittest import mock
import hashlib
import window

class WindowTests(unittest.TestCase):
    def probe(self, folder, logical=(8,6), initial=False):
        rows=[]
        def event(kind,data):
            rows.append({'schema_version':1,'sequence':len(rows)+1,'kind':kind,'data':data})
        event('display-probe-start',{'os':'macos','arch':'aarch64','authored_inputs':True,'stock_or_original_assets_used':False})
        event('gpu-adapter',{'info':'backend: Metal, device_type: IntegratedGpu'})
        event('window-created',{'width':8,'height':6})
        images=[]
        for i,name in enumerate(('initial','toggled','restored')):
            mode=initial if i!=1 else not initial
            width,height=(20,12) if mode else logical
            rect=window.letterbox(logical,(width,height))
            if i: event('display-request',{'fullscreen':mode,'applied':True})
            event('display-layout',{'drawable':[width,height],'logical':list(logical),'fullscreen':mode,'letterbox':rect,'scale_factor':1.0,'monitor':{'size':[20,12]}})
            filename=f'drawable-{name}.png';data=b'decoder-output-'+name.encode();(folder/filename).write_bytes(data)
            event('drawable-captured',{'name':name,'file':filename,'frame_id':i+1,'presents':(10,100,200)[i],'width':width,'height':height,'fullscreen':mode,'letterbox':rect,'rgba_sha256':'a'*64})
            images.append({'name':name,'file':filename,'width':width,'height':height,'authored_pixels_match':True,'png_sha256':hashlib.sha256(data).hexdigest(),'rgba_sha256':'a'*64})
        event('display-script-exit',{'presents':220});event('ui-exit',{'error':None})
        pixels={'schema':1,'authored_inputs':True,'stock_or_original_assets_used':False,'images':images}
        self.write(folder,rows,pixels);return rows,pixels
    def write(self,folder,rows,pixels):
        (folder/'session-00000.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
        (folder/'pixels.json').write_text(json.dumps(pixels))
    def test_complete_metadata_for_both_startup_modes(self):
        for initial in (False,True):
            with tempfile.TemporaryDirectory() as d:
                folder=Path(d);self.probe(folder,initial=initial)
                self.assertTrue(window.validate_probe(folder,(8,6),initial)['complete'])
    def test_mode_requests_cannot_substitute_for_observed_fullscreen(self):
        with tempfile.TemporaryDirectory() as d:
            folder=Path(d);rows,pixels=self.probe(folder)
            next(r for r in rows if r['kind']=='drawable-captured' and r['data']['name']=='toggled')['data']['fullscreen']=False
            self.write(folder,rows,pixels)
            with self.assertRaisesRegex(ValueError,'observed display mode'):window.validate_probe(folder,(8,6),False)
    def test_every_missing_or_failed_observation_is_rejected(self):
        for kind in ('display-probe-start','gpu-adapter','window-created','display-script-exit','ui-exit','drawable-captured'):
            with self.subTest(kind=kind),tempfile.TemporaryDirectory() as d:
                folder=Path(d);rows,pixels=self.probe(folder);rows=[r for r in rows if r['kind']!=kind]
                for i,r in enumerate(rows):r['sequence']=i+1
                self.write(folder,rows,pixels)
                with self.assertRaises(ValueError):window.validate_probe(folder,(8,6),False)
    def test_wrong_geometry_bars_monitor_and_changed_pixels_fail(self):
        for field in ('letterbox','monitor','pixels','file','error','arch'):
            with self.subTest(field=field),tempfile.TemporaryDirectory() as d:
                folder=Path(d);rows,pixels=self.probe(folder)
                if field=='letterbox':next(r for r in rows if r['kind']=='drawable-captured')['data']['letterbox']['x']+=1
                elif field=='monitor':next(r for r in rows if r['kind']=='display-layout' and r['data']['fullscreen'])['data']['monitor']['size']=[19,12]
                elif field=='pixels':pixels['images'][0]['authored_pixels_match']=False
                elif field=='file':(folder/'drawable-initial.png').write_bytes(b'changed')
                elif field=='error':next(r for r in rows if r['kind']=='ui-exit')['data']['error']='GPU failed'
                elif field=='arch':rows[0]['data']['arch']='x86_64'
                self.write(folder,rows,pixels)
                with self.assertRaises(ValueError):window.validate_probe(folder,(8,6),False)
    def test_foreign_output_is_rejected_before_build(self):
        with tempfile.TemporaryDirectory() as d:
            repo=Path(d)/'repo';out=Path(d)/'foreign'
            with mock.patch.object(window,'ROOT',repo),mock.patch.object(window.sys,'platform','darwin'),mock.patch.object(window.platform,'machine',return_value='arm64'),mock.patch.object(window.subprocess,'run') as child:
                got=window.window_result(out)
            self.assertEqual(got['result'],'failed');self.assertFalse(out.exists());child.assert_not_called()
    def test_source_drift_cannot_pass_even_if_all_probe_outputs_validate(self):
        with tempfile.TemporaryDirectory() as d:
            repo=Path(d);tool=mock.Mock();tool.archive_untracked_sources.return_value=[];tool.git_identity.side_effect=[({'commit':'a'},b''),({'commit':'b'},b'')]
            def child(*args,**kwargs):
                binary=repo/'target-platform'/window.TARGET/'debug/examples/display_probe';binary.parent.mkdir(parents=True,exist_ok=True);binary.write_bytes(b'compiled')
                return SimpleNamespace(returncode=0)
            with mock.patch.object(window,'ROOT',repo),mock.patch.object(window.sys,'platform','darwin'),mock.patch.object(window.platform,'machine',return_value='arm64'),mock.patch.object(window,'boot_tools',return_value=tool),mock.patch.object(window.subprocess,'run',side_effect=child),mock.patch.object(window,'validate_probe',return_value={'complete':True}):
                got=window.window_result(repo/'re/out')
            self.assertEqual(got['result'],'stale');self.assertTrue((Path(got['bundle'])/'receipt.json').is_file())

    def test_untracked_source_bytes_are_archived_even_when_build_fails(self):
        real=window.boot_tools()
        with tempfile.TemporaryDirectory() as d:
            repo=Path(d);source=repo/'new-source.py';source.write_bytes(b'authored source bytes')
            identity={'commit':'a','dirty':True,'untracked':[{'path':'new-source.py','bytes':source.stat().st_size,'sha256':hashlib.sha256(source.read_bytes()).hexdigest()}]}
            tool=SimpleNamespace(git_identity=lambda root:(identity,b'patch'),archive_untracked_sources=real.archive_untracked_sources)
            with mock.patch.object(window,'ROOT',repo),mock.patch.object(window.sys,'platform','darwin'),mock.patch.object(window.platform,'machine',return_value='arm64'),mock.patch.object(window,'boot_tools',return_value=tool),mock.patch.object(window.subprocess,'run',return_value=SimpleNamespace(returncode=101)):
                got=window.window_result(repo/'re/out')
            self.assertEqual(got['result'],'failed');bundle=Path(got['bundle']);receipt=json.loads((bundle/'receipt.json').read_text())
            self.assertEqual(len(receipt['untracked_source_archive']),1)
            self.assertEqual((bundle/'source-untracked/new-source.py').read_bytes(),source.read_bytes())
