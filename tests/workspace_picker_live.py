#!/usr/bin/env python3
"""Actual PTY folder-picker flow with a controlled selection and optional KDE dialog cancellation."""
import fcntl,json,os,pty,select,struct,subprocess,sys,tempfile,termios,time,tomllib
from pathlib import Path
from backend_ui_live import Screen
from terminal_smoke import read_until

def run(binary, real=False):
    with tempfile.TemporaryDirectory(prefix="llamancer-picker-") as temp:
        root=Path(temp); cfg=root/"config/llamancer";cfg.mkdir(parents=True)
        project=root/"Projects";project.mkdir();(project/"demo").mkdir()
        (cfg/"config.toml").write_text('icon_mode="plain"\n')
        env=dict(os.environ,TERM="xterm-256color",XDG_CONFIG_HOME=str(cfg.parent),XDG_DATA_HOME=str(root/"data"),LLAMANCER_TEST_FOLDER=str(project))
        if real:
            for line in subprocess.check_output(["systemctl","--user","show-environment"],text=True).splitlines():
                k,_,v=line.partition("=")
                if k in ("DISPLAY","WAYLAND_DISPLAY","XDG_RUNTIME_DIR","DBUS_SESSION_BUS_ADDRESS","XDG_SESSION_TYPE"):env[k]=v
        else:
            stub=root/"bin";stub.mkdir();helper=stub/"kdialog"
            helper.write_text('#!/bin/sh\nsleep 1\nprintf "%s\\n" "$LLAMANCER_TEST_FOLDER"\n')
            helper.chmod(0o700);env["PATH"]=str(stub)+":"+env["PATH"]
        master,slave=pty.openpty();proc=None
        def children():
            ids=set()
            for p in Path(f"/proc/{proc.pid}/task").glob("*/children"):
                try:ids.update(map(int,p.read_text().split()))
                except OSError:pass
            return ids
        try:
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack("HHHH",36,120,0,0))
            original=termios.tcgetattr(slave)
            proc=subprocess.Popen([str(binary)],env=env,stdin=slave,stdout=slave,stderr=slave)
            screen=Screen()
            def wait(x,t=15):screen.wait(master,proc,x.encode(),seconds=t)
            def settle():
                end=time.monotonic()+.25
                while time.monotonic()<end:
                    if select.select([master],[],[],.05)[0]:screen.feed(os.read(master,65536))
            def send(data):os.write(master,data);settle()
            def click(marker):
                wait(marker);settle()
                for y,row in enumerate(screen.rows):
                    row="".join(row)
                    if marker in row:
                        x=row.index(marker)+len(marker)//2
                        send(f"\x1b[<0;{x+1};{y+1}M\x1b[<0;{x+1};{y+1}m".encode());return
                raise AssertionError(marker)
            wait("LLAMANCER");send(b"5");click("Workspace");click("Browse folder");wait("Folder selection")
            # A real picker child must exist before we test cancel.
            if real:
                deadline=time.monotonic()+5
                ids=set()
                while time.monotonic()<deadline:
                    ids={i for i in children() if Path(f"/proc/{i}/comm").exists() and Path(f"/proc/{i}/comm").read_text().strip()=="kdialog"}
                    if ids:break
                    time.sleep(.1)
                assert ids,"KDialog did not start in the desktop session"
                send(b"2");assert "Folder selection" in "\n".join("".join(r) for r in screen.rows)
                send(b"\x1b");wait("Folder selection cancelled")
                assert all(not Path(f"/proc/{i}").exists() for i in ids)
                assert "workspace_root" not in tomllib.loads((cfg/"config.toml").read_text())
                print("Real KDE KDialog open, modal isolation, cancellation and child cleanup passed")
            else:
                wait("Workspace saved")
                conf=tomllib.loads((cfg/"config.toml").read_text());assert conf["workspace_root"]==str(project)
                click("Projects");wait("Project workspace");wait("demo");send(b"\r")
                conf=tomllib.loads((cfg/"config.toml").read_text());assert conf["project_dir"]==str(project/"demo")
                # Cancel a second explicitly started dialog before it can return a result.
                helper.write_text('#!/bin/sh\nexec /usr/bin/sleep 30\n')
                click("Browse folder");wait("Folder selection")
                send(b"2");assert "Folder selection" in "\n".join("".join(r) for r in screen.rows)
                send(b"\x1b");wait("Folder selection cancelled")
                assert tomllib.loads((cfg/"config.toml").read_text())==conf
                print("Mouse folder selection, saved workspace/project, modal isolation and unchanged settings on cancellation passed")
            os.write(master,b"q");read_until(master,proc,b"\x1b[?1049l");assert proc.wait(timeout=10)==0
            assert termios.tcgetattr(slave)==original
        finally:
            if proc and proc.poll() is None:proc.terminate();proc.wait(timeout=15)
            os.close(master);os.close(slave)

if __name__=="__main__":
    binary=Path(sys.argv[1]).resolve(strict=True)
    run(binary)
    if "--real" in sys.argv[2:]:run(binary,True)
