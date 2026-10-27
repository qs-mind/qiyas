import os,sys,time,subprocess
def procs():
    out=subprocess.run(["ps","-eo","pid,args"],capture_output=True,text=True).stdout.splitlines()
    r={}
    for l in out:
        pid,_,a=l.strip().partition(" ")
        if any(k in a for k in sys.argv[2:]) and "cpu.py" not in a: r[int(pid)]=a.split("/")[-1][:40]
    return r
def ticks(p):
    try:
        f=open(f"/proc/{p}/stat").read().rsplit(")",1)[1].split(); return int(f[11])+int(f[12])
    except: return None
def rss(p):
    try:
        for l in open(f"/proc/{p}/status"):
            if l.startswith("VmRSS"): return int(l.split()[1])//1024
    except: return None
secs=float(sys.argv[1]); P=procs(); a={p:ticks(p) for p in P}; time.sleep(secs); hz=os.sysconf("SC_CLK_TCK")
tot=0
for p,name in P.items():
    b=ticks(p)
    if b is None or a[p] is None: continue
    cpu=(b-a[p])/hz/secs*100; tot+=cpu; print(f"{name:42s} pid {p} cpu {cpu:5.1f}%  rss {rss(p)} MB")
print(f"total cpu {tot:.1f}% of one core over {secs:.0f} s")
