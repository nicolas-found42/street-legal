"""Exact rational error majorants for pinned standalone model, never oracle values.

Reference expression is true sin/cos(x). Uses rational alternating-series
remainders, exact parsed binary128 constants, the actual 64/67-bit DAG, and
66-bit Pi reduction. This proves the named model's error, not model/hardware
identity or a raw scalar image. All arithmetic in proof is fractions.Fraction.
"""
from fractions import Fraction as F
from pathlib import Path
from math import factorial
import re,json,hashlib
ROOT=Path(__file__).resolve().parents[2];SOURCE=ROOT/'crates/slr-x87/src/intel_trig.rs';text=SOURCE.read_text()
UPSTREAM_PIN='d245c751e6a6fab4bf68e72823598a48001adb3c'
UPSTREAM_SOURCE_PINS={
 'bochs/cpu/fpu/fsincos.cc':'4544bf8d862188edff834fd88a3186ad00360426d068f0dcecae07ff072c99ff',
 'bochs/cpu/fpu/f128_67.cc':'b150bae357f60f71185cd06bdee03d3e138d74701d3d00c63ff444ca44f05c4e',
 'bochs/cpu/fpu/fsincos_constants.h':'4e260a21afcbd3346dec4688b39914349769fb33b3c3817a876726050b31169f',
}

MODEL_SOURCE_PIN='e1d6c82386bc5db8579002409e21bd20818309c62b72c3f18e2b4686b913594e'
assert hashlib.sha256(SOURCE.read_bytes()).hexdigest()==MODEL_SOURCE_PIN, 'model DAG changed: redo proof review'

def power(e):return F(2)**e
def decode(hi,lo):
 s=-1 if hi>>63 else 1;e=(hi>>48)&32767;sig=(1<<112)|((hi&((1<<48)-1))<<64)|lo
 return s*F(sig)*power(e-16383-112)
def array(name):
 body=re.search(r'const '+name+r': \[Quad; \d+\] = \[(.*?)\n\];',text,re.S).group(1)
 return [decode(int(a,16),int(b,16)) for a,b in re.findall(r'hi: (0x[0-9A-Fa-f]+),\s*lo: (0x[0-9A-Fa-f]+)',body)]
A,B,U,V,SIN,COS=[array(n) for n in ['A','B','U','V','SIN','COS']]
# Independent exactpi enclosure (Machin), endpoints certified by alternating
# arctanseries residual. This does not assume a binary64 mathlibrary.
def atan(q,n=96):
 v=sum(((-1)**k*F(q)**(2*k+1)/F(2*k+1) for k in range(n)),F(0));r=F(q)**(2*n+1)/F(2*n+1)
 return (v,v+r) if n%2==0 else (v-r,v)
a,b=atan(F(1,5)),atan(F(1,239));pi=(16*a[0]-4*b[1],16*a[1]-4*b[0]);Pi=F(int('c90fdaa22168c234c',16),1<<66)
phase=3*max(abs(Pi-pi[0]),abs(Pi-pi[1])) #6(Pi/2-pi/2)
assert F(3)<pi[0]<pi[1]<F(22,7);assert phase<power(-65)
# |x|<=8 -> nearestinteger reduction byPi/2 uses|n|<=6. It leaves
# |r|<=Pi/4<1, and integerquotient/remainder retains allbits. FLDm32 and
# the66-bitdivisor yield atmost66significantremainderbits, exact inbinary128.

# Eachbound tracks absoluteidealvalue, and distance between implementation
# arithmetic and its exactreal polynomial. The exact multiplication DAG's
# error identity is |a db+b da+da db|. Additions use triangleinequality.
# Rounding: binary128 operation then67-bit chop <=2*2^(1-67)*|result|
# (rounding up to next binade included) =2^-65*bound. Final64-bit nearest
# <=2^-63*bound, conservatively covering binary128's precedinground.
class E:
 def __init__(self,v,e=F(0)):self.v=F(v);self.e=F(e)
 def actual(self):return self.v+self.e
 def add(self,b,p):
  v=self.v+b.v;e=self.e+b.e
  return E(v,e+(v+e)*power(2-p))
 def mul(self,b,p,chop=False):
  eb=b.e+(b.actual()*power(-63) if chop else 0)
  e=self.v*eb+b.v*self.e+self.e*eb;v=self.v*b.v
  return E(v,e+(v+e)*power(2-p))
def C(q):return E(abs(q))
def add67(a,b):return a.add(b,67)
def add64(a,b):return a.add(b,65) #p65 ->2^-63, sameconservative64-round bound
# 67-bit rounding majorant includes binary128 intermediate.
def mul67(a,b):return a.mul(b,67)
def mule67(a,b):return a.mul(b,67,True)
def mul64(a,b):return a.mul(b,65,True)
def finish(a,b):return add64(a,b)

def small(cosine):
 r=E(F(1,4));s=mule67(r,r);t=mule67(s,s);co=B if cosine else A
 p=mul67(t,C(co[4]));p=add64(p,C(co[2]));p=mul67(t,p);p=add64(p,C(co[0]));p=mul67(s,p)
 q=mul67(t,C(co[5]));q=add64(q,C(co[3]));q=mul67(t,q);q=add64(q,C(co[1]));q=mul67(t,q)
 y=finish(C(1),add67(p,q)) if cosine else finish(r,mul67(r,add64(p,q)))
 # Exactpolynomial coefficient deviation fromTaylor plusnextalternatingterm.
 extra=0 if cosine else 1
 approx=sum((abs(c-F((-1)**(k+1),factorial(2*k+2+extra)))*F(1,4)**(2*k+2+extra) for k,c in enumerate(co)),F(0))
 approx+=F(1,4)**(14+extra)/factorial(14+extra)
 return y.e,approx

# Tablelaneideal d obeys|d|<=1/16, c values followindexmapping exactly.
centers=[F(9+2*i,16) for i in range(4)]+[F(9+2*i,32) for i in range(4)]
def trig_interval(x,cosine,n=32):
 s=sum((F((-1)**k)*x**(2*k+int(not cosine))/factorial(2*k+int(not cosine)) for k in range(n)),F(0));r=x**(2*n+int(not cosine))/factorial(2*n+int(not cosine))
 return (s,s+r) if n%2==0 else (s-r,s)
table_errors=[]
for i,c in enumerate(centers):
 for values,cosine in [(SIN,False),(COS,True)]:
  lo,hi=trig_interval(c,cosine);table_errors.append(max(abs(values[i]-lo),abs(values[i]-hi)))
table_error=max(table_errors)
d=F(1,16);p_error=sum((abs(u-F((-1)**(k+1),factorial(2*k+3)))*d**(2*k+3) for k,u in enumerate(U)),F(0))+d**11/factorial(11)
q_error=sum((abs(v-F((-1)**(k+1),factorial(2*k+2)))*d**(2*k+2) for k,v in enumerate(V)),F(0))+d**10/factorial(10)
# Tableexactexpression sin(c)*(1+q)+cos(c)*p orcos(c)*(1+q)-sin(c)*p.
# |sin/cos(c)|<=1; errors from bothcoeffpolys add. Eachstoredtableconstant
# differs from truefunction bytable_error, amplifiedby1+|q|+|p|<2.
table_approx=p_error+q_error+2*table_error

def tab(cosine):
 # |r|,|c|<1. add67's cancellationerror <=(1+1)*2^-65.
 d=E(F(1,16),2*power(-65));e=mul67(d,d);p=C(U[3]);q=C(V[3])
 for i in range(2,-1,-1):p=add64(mul67(e,p),C(U[i]));q=add64(mul67(e,q),C(V[i]))
 p=add64(d,mul67(mul67(e,p),d));q=mul64(e,q)
 y=finish(E(1),add67(mul67(E(1),p),mul67(E(1),q)))
 return y.e,table_approx
results={}
for lane,fn in [('small',small),('table',tab)]:
 for cosine in [False,True]:
  round_error,approx_error=fn(cosine);total=phase+round_error+approx_error
  assert total<power(-56)
  results[lane+('_cos' if cosine else '_sin')]={ 'rounding_error':str(round_error),'approximation_error':str(approx_error),'phase_error':str(phase),'total_error':str(total),'total_error_float':float(total),'conservative_budget':str(power(-56)), 'budget_ratio_float':float(total/power(-56))}
receipt=dict(schema='standalone-model-rational-error-v1',upstream_pin=UPSTREAM_PIN,upstream_source_pins=UPSTREAM_SOURCE_PINS,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),input_domain='finite m32,|x|<=8,CWnearest/masked',reference='exactreal sin/cos(x)',proof='exactrational majorants; modelidentity withoriginalscalarhardware NOT established',table_error_float=float(table_error),results=results)
print(json.dumps(receipt,indent=2))
