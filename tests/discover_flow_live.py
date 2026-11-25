#!/usr/bin/env python3
"""Opt-in real public Hub -> double-click -> auto download -> Library -> run test."""
import fcntl,json,os,pty,select,socket,struct,subprocess,sys,tempfile,time,tomllib,termios
from pathlib import Path
from backend_ui_live import Screen
from terminal_smoke import read_until

def main():
 binary=Path(sys.argv[1]).resolve(strict=True)
 backend=Path(sys.argv[2]).resolve(strict=True)
 with tempfile.TemporaryDirectory(prefix="llamancer-flow-") as scratch:
  root=Path(scratch);cfg=root/"config/llamancer";cfg.mkdir(parents=True)
  with socket.socket() as sock:
   sock.bind(("127.0.0.1",0));port=sock.getsockname()[1]
  (cfg/"config.toml").write_text(f"server_binary={json.dumps(str(backend))}\ndevice=\"none\"\nendpoint=\"http://127.0.0.1:{port}\"\n")
  master,slave=pty.openpty();proc=None
  try:
   fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",36,120,0,0))
   env=dict(os.environ,TERM="xterm-256color",XDG_CONFIG_HOME=str(cfg.parent),XDG_DATA_HOME=str(root/"data"),HF_HUB_DISABLE_IMPLICIT_TOKEN="1")
   proc=subprocess.Popen([str(binary)],stdin=slave,stdout=slave,stderr=slave,env=env)
   screen=Screen()
   def wait(s,t=15):screen.wait(master,proc,s.encode(),seconds=t)
   def send(b):os.write(master,b);time.sleep(.12)
   def render():return "\n".join("".join(row) for row in screen.rows)
   wait("LLAMANCER");send(b"G");wait("Guided setup");send(b"\r");wait("Edit settings");wait("default");send(b"\r");wait("Scan complete");assert (root/"data/llamancer/models").is_dir()
   send(b"2");send(b"@ggml-org/tiny-llamas");send(b"\r");wait("variants · Enter to download",35)
   # Click the visible filter field (body row 1; one-based terminal coordinates).
   send(b"\x1b[<0;5;7M\x1b[<0;5;7m");send(b"stories260K.gguf");send(b"\r");wait("stories260K.gguf")
   # Wait for the actual selected row, not the filter text.
   deadline=time.monotonic()+5
   while time.monotonic()<deadline:
    if select.select([master],[],[],.1)[0]:screen.feed(os.read(master,65536))
    rows=[i for i,r in enumerate(screen.rows) if "> " in "".join(r) and "stories260K.gguf" in "".join(r)]
    if rows:break
   assert len(rows)==1,render()
   y=rows[0]+1
   click=f"\x1b[<0;5;{y}M\x1b[<0;5;{y}m".encode()
   os.write(master,click+click);wait("Download destination")
   send(b"\r");wait("Complete",40)
   until=time.monotonic()+15
   while time.monotonic()<until:
    if select.select([master],[],[],.1)[0]:screen.feed(os.read(master,65536))
    q=tomllib.loads((cfg/"downloads.toml").read_text())
    if q["items"][0]["state"]=="Complete":break
   assert q["items"][0]["state"]=="Complete" and q["items"][0]["receipt"]
   send(b"1");wait("stories260K",20)
   send(b"l");wait("Server",10)
   import urllib.request
   until=time.monotonic()+45
   while time.monotonic()<until:
    try:
     with urllib.request.urlopen(f"http://127.0.0.1:{port}/health",timeout=1) as r:
      if r.status==200:break
    except OSError:time.sleep(.25)
    if select.select([master],[],[],0)[0]:screen.feed(os.read(master,65536))
   else:raise AssertionError(render())
   req=urllib.request.Request(f"http://127.0.0.1:{port}/completion",data=json.dumps({"prompt":"Once upon a time","n_predict":4,"temperature":0.0}).encode(),headers={"Content-Type":"application/json"})
   with urllib.request.urlopen(req,timeout=30) as r:assert json.load(r)["content"]
   send(b"q");read_until(master,proc,b"\x1b[?1049l");assert proc.wait(timeout=10)==0
   with socket.socket() as sock:
    sock.settimeout(1)
    assert sock.connect_ex(("127.0.0.1",port))!=0, "Listener remained active"
    sock.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
    sock.bind(("127.0.0.1",port))
   print("Real guided default folder, Discover filter, double-click download, automatic verified transfer, Library rescan, CPU inference and cleanup passed.")
  finally:
   if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=30)
   os.close(master);os.close(slave)
if __name__=="__main__":main()
