#!/usr/bin/env python3
"""Opt-in real PTY: visible mouse controls, context persistence/restart and theme selector."""
import fcntl,json,os,pty,select,socket,struct,subprocess,sys,tempfile,time,tomllib,termios,urllib.request
from pathlib import Path
from backend_ui_live import Screen
from terminal_smoke import read_until

def main():
    binary,backend,model=(Path(v).resolve(strict=True) for v in sys.argv[1:])
    with tempfile.TemporaryDirectory(prefix="llamancer-visual-") as tmp:
        root=Path(tmp); cfg=root/"config/llamancer"; cfg.mkdir(parents=True)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1",0));port=sock.getsockname()[1]
        endpoint=f"http://127.0.0.1:{port}"
        (cfg/"config.toml").write_text(f"model_imports=[{json.dumps(str(model))}]\nserver_binary={json.dumps(str(backend))}\nendpoint={json.dumps(endpoint)}\ndevice=\"none\"\n")
        m,s=pty.openpty();fcntl.ioctl(s,termios.TIOCSWINSZ,struct.pack("HHHH",36,120,0,0))
        p=subprocess.Popen([str(binary)],env=dict(os.environ,TERM="xterm-256color",XDG_CONFIG_HOME=str(cfg.parent),XDG_DATA_HOME=str(root/"data"),HF_HUB_DISABLE_IMPLICIT_TOKEN="1"),stdin=s,stdout=s,stderr=s)
        screen=Screen()
        def wait(marker,t=15):screen.wait(m,p,marker.encode(),seconds=t)
        def settle():
            until=time.monotonic()+.3
            while time.monotonic()<until:
                if select.select([m],[],[],.05)[0]:screen.feed(os.read(m,65536))
        def send(data):os.write(m,data);settle()
        def click(marker,last=False,double=False):
            wait(marker);settle()
            rows=[(y,"".join(r)) for y,r in enumerate(screen.rows) if marker in "".join(r)]
            y,row=rows[-1 if last else 0];x=row.index(marker)+len(marker)//2
            b=f"\x1b[<0;{x+1};{y+1}M\x1b[<0;{x+1};{y+1}m".encode()
            send(b*(2 if double else 1))
        def api(path):
            with urllib.request.urlopen(endpoint+path,timeout=1) as r:return json.load(r)
        def check_context(n):
            until=time.monotonic()+30;last={}
            while time.monotonic()<until:
                try:
                    props=api("/props")
                    last=props.get("default_generation_settings",{})
                    models=api("/v1/models").get("data",[])
                    reported=[last.get("n_ctx")]+[x.get("meta",{}).get("n_ctx") for x in models if isinstance(x.get("meta"),dict)]
                    if n in reported:return
                except (OSError,ValueError):pass
                settle()
            raise AssertionError(f"server did not report context {n}: {str(last)[:300]}\n"+"\n".join("".join(r) for r in screen.rows))
        try:
            wait("Scan complete");click("Indexed", double=True);wait("Launch model")
            assert "Advanced settings" in "\n".join("".join(r) for r in screen.rows)
            click("Presets");wait("Context size (tokens)");click("Custom value");send(b"4096\r")
            wait("Context for this model: 4096")
            conf=tomllib.loads((cfg/"config.toml").read_text());assert conf["profiles"][str(model)]["engine"]["context_size"]==4096
            click("Advanced settings");wait("OMP thinking:")
            click("Advanced settings");click("Run model only");check_context(4096)
            wait("Connected");wait("Stop server")
            # Click Library tab, reopen same model and change context without stopping manually.
            send(b"1");click("Run model");click("Presets");click("8192",last=True);wait("Context for this model: 8192")
            click("Run model only");check_context(8192)
            wait("Connected");wait("Stop server");click("Stop server")
            wait("Managed server stopped")
            with socket.socket() as sock:sock.settimeout(1);assert sock.connect_ex(("127.0.0.1",port))!=0
            send(b"5");click("Appearance");click("Appearance  Theme",double=True);wait("Choose a value")
            click("catppuccin-mocha");wait("Settings saved")
            assert tomllib.loads((cfg/"config.toml").read_text())["theme"]=="catppuccin-mocha"
            send(b"2");click("Sort:");wait("Sort by");send(b"\x1b")
            click("Filters");wait("Filter models");click("Quantization");wait("Quantization (repository variants)");send(b"\x1b")
            send(b"\t");os.write(m,b"q");read_until(m,p,b"\x1b[?1049l");assert p.wait(timeout=10)==0
            print("Library row double-click, launch, presets, context restart (actual API context), stop, theme and discovery selectors passed.")
        finally:
            if p.poll() is None:p.terminate();p.wait(timeout=20)
            os.close(m);os.close(s)
if __name__=="__main__":main()
