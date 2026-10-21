#!/usr/bin/env python3
"""Passive request, worker, shape completion and decode scheduling observations."""
import startup

class FileWorkerObserver(startup.Observer):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        self.file_worker_events=[]
        self.file_worker_total=0
        self.resource_returns={}
        self.return_sites=set()
        for address in [0x554ea0,0x5553b0,0x555470,0x5550ee,0x555209,0x505c00,0x501a70,0x501b90,0x505db0,0x505e80,0x419860,0x537240,0x54a790,0x53b710]:
            self.hook(address,self.observe_file_worker)
    def observe_file_worker(self,uc,address,size,user):
        self.file_worker_total+=1
        if len(self.file_worker_events)>=20000:return
        sp=self.reg('esp')
        event=dict(icount=self.host.icount,pc=address,thread=self.host.thread_id,frame=self.loop_iterations,generation=self.try_u32(0x6200a4),stack=[self.try_u32(sp+i*4) for i in range(9)],eax=self.reg('eax'),ecx=self.reg('ecx'),esi=self.reg('esi'),edi=self.reg('edi'),pending=self.try_u32(0x65c204),ready=self.try_u32(0x65c208),current_shape=self.try_u32(0x65c1d8),native_count=self.stats['native_calls'])
        if address in [0x5553b0,0x555470]:
            pointer=self.try_u32(sp+4)
            event['path']=self.cstr(pointer,256)
        if address in [0x419860,0x537240,0x54a790,0x53b710]:
            resource=self.try_u32(sp+4) if address==0x537240 else self.reg('ecx')
            event['resource']=self.resource(resource)
            if address==0x419860:
                ret=self.try_u32(sp)
                self.resource_returns[(self.host.thread_id,ret,sp+20)]=resource
                if ret not in self.return_sites:
                    self.hook(ret,self.observe_resource_return)
                    self.return_sites.add(ret)
        self.file_worker_events.append(event)
    def resource(self,p):
        return dict(address=p,**{name:self.try_u32(p+offset) for name,offset in [('id',0x50),('type',0x4c),('flags',0x54),('state',0x64),('quality',0x68),('requested_quality',0x6c)]}) if p else None
    def observe_resource_return(self,uc,address,size,user):
        key=(self.host.thread_id,address,self.reg('esp'))
        resource=self.resource_returns.pop(key,None)
        if resource is None:return
        self.file_worker_total+=1
        if len(self.file_worker_events)<20000:
            self.file_worker_events.append(dict(icount=self.host.icount,pc=address,event='419860_return',thread=self.host.thread_id,frame=self.loop_iterations,generation=self.try_u32(0x6200a4),result=self.reg('eax'),resource=self.resource(resource),native_count=self.stats['native_calls']))
    def sidecar(self):
        return {**super().sidecar(),'file_worker':dict(alters_execution=False,events=self.file_worker_events,total=self.file_worker_total,truncated=self.file_worker_total>len(self.file_worker_events))}

if __name__=='__main__':
    startup.Observer=FileWorkerObserver
    startup.main()
