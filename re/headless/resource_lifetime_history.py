"""Bounded passive original resource queues, destructors and linked references."""

class ResourceLifetimeHistory:
 def __init__(self, observer, start=95_000_000, end=110_000_000, limit=5000):
  if limit < 1 or end < start: raise ValueError("invalid resource lifetime observation bounds")
  self.o=observer;self.start=start;self.end=end;self.limit=limit;self.lifetime=[];self.lifetime_total=0
  for address in [0x48a8d0,0x48a8f0,0x427620,0x427720,0x4277f0,0x544700,0x544e20,0x545f70,0x5517f0,0x536f90]:self.o.hook(address,self.life)
 def words(self,p,n):return [self.o.try_u32(p+i*4) for i in range(n)] if p else None
 def resource(self,p):
  return {'address':p,'vtable':self.o.try_u32(p),'id':self.o.try_u32(p+0x50),'type':self.o.try_u32(p+0x4c),'flags':self.o.try_u32(p+0x54),'load_state':self.o.try_u32(p+0x64),'head':self.o.try_u32(p+0x48),'parent':self.o.try_u32(p+0x14),'first_child':self.o.try_u32(p+0x20)} if p else None
 def links(self,p,kind):
  rows=[];seen=set()
  while p and p not in seen and len(rows)<64:
   seen.add(p);n=self.o.try_u32(p+4)
   if kind=='queue' and not n:break
   rows.append({'address':p,'words':self.words(p,7 if kind=='queue' else 4)})
   p=n
  return rows
 def life(self,uc,address,size,user):
  if not self.start<=self.o.host.icount<=self.end:return
  self.lifetime_total+=1
  if len(self.lifetime)>=self.limit:return
  p=self.o.reg('ecx');sp=self.o.reg('esp');e={'pc':address,'icount':self.o.host.icount,'frame':self.o.loop_iterations,'this':p,'stack':self.words(sp,12),'native_count':self.o.stats['native_calls']}
  if address in (0x48a8d0,0x48a8f0):e['reference']=self.words(p,4);e['resource']=self.resource(self.o.try_u32(p+12))
  if address in (0x427620,0x427720):
   ref=self.o.try_u32(sp+4);e['reference']=self.words(ref,4);e['resource']=self.resource(self.o.try_u32(ref+12))
  if address==0x4277f0:e['queues']={hex(offset):self.links(self.o.try_u32(p+offset),'queue') for offset in (0x5c,0x78,0xb0)}
  if address in (0x544700,0x544e20,0x5517f0):e['resource']=self.resource(p);e['head_links']=self.links(self.o.try_u32(p+0x48),'head')
  if address==0x545f70:e['resource']=self.resource(p-0x44);e['head_links']=self.links(self.o.try_u32(p+4),'head')
  self.lifetime.append(e)
 def sidecar(self):
  return {'alters_execution':False,'events':self.lifetime,'total':self.lifetime_total,'truncated':self.lifetime_total>len(self.lifetime),'start':self.start,'end':self.end,'limit':self.limit}
