#!/usr/bin/env python3
"""Run unchanged startup with passive render-generation observations.

Records original counter values at native car/template entry and normal/flush
increment instructions. It does not replace instructions, inputs or outputs.
"""
import startup

class GenerationObserver(startup.Observer):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        self.generations=[]
        for address in [0x442500,0x44a250,0x428d91,0x428da0,0x47c209,0x47c309]:
            self.hook(address,self.observe_generation)
    def observe_generation(self,uc,address,size,user):
        if len(self.generations)<4096:
            self.generations.append(dict(icount=self.host.icount,address=hex(address),generation=self.try_u32(0x6200a4),loop_iteration=self.loop_iterations,thread=self.host.thread_id))
    def sidecar(self):
        return {**super().sidecar(),'render_generations':dict(alters_execution=False,events=self.generations)}

if __name__=='__main__':
    startup.Observer=GenerationObserver
    startup.main()
