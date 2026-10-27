#!/usr/bin/env python3
"""launch.py <label> <runs> [idle_seconds]: starts the real qiyas-desktop binary on a headless Broadway display
(no window on any screen), behind the timing proxy on :1420, and prints when each step happened."""
import os, subprocess, sys, time, signal, re
SP = os.environ.get("PERF_WORK", "/tmp/qiyas-perf")  # logs, the app's isolated XDG folders, cpu.py
os.makedirs(SP, exist_ok=True)
HERE = os.path.dirname(os.path.abspath(__file__))
label, runs = sys.argv[1], int(sys.argv[2]); idle = float(sys.argv[3]) if len(sys.argv) > 3 else 0
binary = os.environ.get("QIYAS_BIN", os.path.join(HERE, "../../target/debug/qiyas-desktop"))
logf = f"{SP}/proxy-{label}.log"
env = dict(os.environ, XDG_CONFIG_HOME=f"{SP}/xdg/config", XDG_DATA_HOME=f"{SP}/xdg/data", XDG_CACHE_HOME=f"{SP}/xdg/cache",
           GDK_BACKEND="broadway", BROADWAY_DISPLAY=":5", WEBKIT_DISABLE_COMPOSITING_MODE="1", WEBKIT_DISABLE_DMABUF_RENDERER="1", GSK_RENDERER="cairo")
bw = subprocess.Popen(["broadwayd", ":5"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); time.sleep(1)
def tree_rss(root):
    out = subprocess.run(["ps", "-eo", "pid,ppid,rss"], capture_output=True, text=True).stdout.splitlines()[1:]
    kids, rss_of = {}, {}
    for l in out:
        pid, ppid, rss = map(int, l.split()); kids.setdefault(ppid, []).append(pid); rss_of[pid] = rss
    tot, stack = 0, [root]
    while stack:
        p = stack.pop(); tot += rss_of.get(p, 0); stack += kids.get(p, [])
    return tot // 1024
try:
    for i in range(runs):
        open(logf, "w").close()
        t0 = time.time()
        app = subprocess.Popen(["nice", "-n", "15", binary], env=env, cwd=SP, stdout=open(f"{SP}/app-{label}.log", "w"), stderr=subprocess.STDOUT, preexec_fn=os.setsid)
        marks, deadline = {}, t0 + 90
        while time.time() < deadline:
            time.sleep(0.05)
            for l in open(logf).read().splitlines():
                ts, _, rest = l.partition(" ")
                if rest.startswith("GET /") and "first_get" not in marks: marks["first_get"] = int(ts) - t0 * 1000
                if rest.startswith("MARK ") and rest[5:] not in marks: marks[rest[5:]] = int(ts) - t0 * 1000
            if any(k.startswith("ready-") for k in marks): break
        time.sleep(1.0)
        row = {}
        for k, v in marks.items():
            name = re.sub(r"-\d+$", "", k); row[name] = round(v)
        row["gets"] = sum(1 for l in open(logf) if " GET " in l)
        row["tree_rss_MB"] = tree_rss(app.pid)
        if idle:
            time.sleep(float(os.environ.get("IDLE_WAIT", "2")))
            r = subprocess.run(["python3", f"{HERE}/cpu.py", str(idle), "qiyas-desktop", "WebKitWebProcess"], capture_output=True, text=True).stdout
            row["idle"] = " | ".join(x.split(" pid")[0].strip() + x.split("pid")[1].split("rss")[0][-12:] for x in r.strip().splitlines() if "qiyas-desktop" in x or "WebKitWeb" in x and "pid" in x) + " | " + r.strip().splitlines()[-1]
        print(label, i + 1, row, flush=True)
        os.killpg(app.pid, signal.SIGTERM); time.sleep(2)
finally:
    bw.terminate()
