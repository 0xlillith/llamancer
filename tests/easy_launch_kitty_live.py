#!/usr/bin/env python3
"""Real two-Enter Library -> llama-server -> independent Kitty OMP window test."""
import json, os, socket, subprocess as sp, tempfile, time, urllib.request, signal, sys, tomllib
from pathlib import Path
binary,server,model=(Path(x).resolve(strict=True) for x in sys.argv[1:])
gui=dict(os.environ)
for line in sp.check_output(["systemctl","--user","show-environment"],text=True).splitlines():
 k,_,v=line.partition("=")
 if k in ("DISPLAY","WAYLAND_DISPLAY","XDG_RUNTIME_DIR","DBUS_SESSION_BUS_ADDRESS","XDG_SESSION_TYPE"):gui[k]=v
with tempfile.TemporaryDirectory(prefix="llamancer-two-enter-") as tmp:
 root=Path(tmp);home=root/"home";cfg=root/"config/llamancer";project=root/"project"
 for p in (home,cfg,project):p.mkdir(parents=True)
 with socket.socket() as s:s.bind(("127.0.0.1",0));port=s.getsockname()[1]
 endpoint=f"http://127.0.0.1:{port}"
 (cfg/"config.toml").write_text(f"model_imports=[{json.dumps(str(model))}]\nserver_binary={json.dumps(str(server))}\nendpoint={json.dumps(endpoint)}\ndevice=\"none\"\nproject_dir={json.dumps(str(project))}\ndefault_agent=\"omp\"\nagent_terminal=\"current\"\n")
 env={k:v for k,v in gui.items() if k in ("PATH","DISPLAY","WAYLAND_DISPLAY","XDG_RUNTIME_DIR","DBUS_SESSION_BUS_ADDRESS","XDG_SESSION_TYPE","LANG","LC_ALL")}
 env.update(HOME=str(home),XDG_CONFIG_HOME=str(cfg.parent),XDG_DATA_HOME=str(root/"data"),NO_PROXY="localhost,127.0.0.1")
 for setting,value in (("startup.setupWizard","false"),("input.bareExitOnEmptySession","true"),("theme.dark","titanium")):
  sp.run(["omp","config","set",setting,value],env=env,cwd=project,check=True,stdout=sp.DEVNULL,timeout=15)
 remote="unix:"+str(root/"kitty.sock")
 app=sp.Popen(["kitty","--config","NONE","--override","allow_remote_control=socket-only","--listen-on",remote,"--title","Llamancer launch acceptance",str(binary)],env=env,cwd=project,stdout=sp.DEVNULL,stderr=sp.DEVNULL)
 def rc(*a):return sp.check_output(["kitten","@","--to",remote,*a],env=gui,text=True,stderr=sp.DEVNULL,timeout=5)
 def wait(test,desc,t=35):
  until=time.monotonic()+t
  while time.monotonic()<until:
   if app.poll() is not None:raise RuntimeError("Llamancer window exited")
   try:
    result=test()
    if result:return result
   except (OSError,sp.SubprocessError,ValueError):pass
   time.sleep(.2)
  raise AssertionError(desc)
 def screen(s):return s in rc("get-text")
 def matches():
  for p in Path("/proc").iterdir():
   if not p.name.isdigit():continue
   try:
    if os.readlink(p/"cwd")!=str(project):continue
    argv=(p/"cmdline").read_bytes().split(b"\0");envs=(p/"environ").read_bytes().split(b"\0")
    if argv and b"omp" in argv[0] and any(v.startswith(b"LLAMA_CPP_BASE_URL=") and str(port).encode() in v for v in envs):yield int(p.name)
   except OSError:continue
 def find_agent():return list(matches())
 try:
  wait(lambda:screen("Scan complete"),"model scan")
  rc("send-key","enter");wait(lambda:screen("Launch model"),"launch menu")
  # Save a preferred global context and a model override from the launch menu.
  rc("send-key","down","down","down","down");rc("send-key","enter");rc("send-text","4096");rc("send-key","enter")
  wait(lambda:screen("Default context (all models): 4096"),"saved default context")
  rc("send-key","up");rc("send-key","enter");rc("send-text","2048");rc("send-key","enter")
  wait(lambda:screen("Context for this model: 2048"),"saved override")
  config=tomllib.loads((cfg/"config.toml").read_text())
  assert config["default_context"]==4096
  assert config["profiles"][str(model)]["engine"]["context_size"]==2048
  rc("send-key","up","up","up");rc("send-key","enter")
  pids=wait(find_agent,"real OMP launched with exact endpoint",60)
  assert pids
  with urllib.request.urlopen(endpoint+"/health",timeout=3) as r:assert r.status==200
  wait(lambda:screen("Server"),"Llamancer still visible")
  rc("send-text","1");wait(lambda:screen("Library"),"Llamancer responsive while OMP open")
  rc("send-text","4");wait(lambda:screen("Server"),"server tab")
  print("Actual OMP process with selected endpoint and isolated project, independent Kitty window, live Llamancer navigation passed",flush=True)
  for pid in pids:
   try:os.kill(pid,signal.SIGTERM)
   except ProcessLookupError:pass
  wait(lambda:not find_agent(),"agent exit")
  def port_closed():
   with socket.socket() as s:s.settimeout(.5);return s.connect_ex(("127.0.0.1",port))!=0
  wait(port_closed,"agent exit releases owned server",25)
  print("Closing OMP releases its managed inference before Llamancer exits",flush=True)
  # A model-only session is independent and must survive the agent window.
  rc("send-text","1");wait(lambda:screen("Library"),"library")
  rc("send-key","enter");wait(lambda:screen("Launch model"),"launch menu")
  rc("send-key","down");rc("send-key","enter")
  wait(lambda:screen("Connected"),"manual model ready",35)
  rc("send-key","enter")
  pids=wait(find_agent,"agent on independently started server")
  for pid in pids:
   try:os.kill(pid,signal.SIGTERM)
   except ProcessLookupError:pass
  wait(lambda:not find_agent(),"second agent exit")
  time.sleep(2)
  assert not port_closed(),"independent server must stay running"
  rc("send-text","x");wait(port_closed,"manual stop")
  rc("send-text","q");app.wait(timeout=20)
  print("Independent server survives agent close; explicit stop and context persistence passed",flush=True)
 finally:
  for pid in matches():
   try:os.kill(pid,signal.SIGTERM)
   except ProcessLookupError:pass
  if app.poll() is None:
   try:rc("send-key","ctrl+c");app.wait(timeout=15)
   except Exception:app.terminate();app.wait(timeout=10)
