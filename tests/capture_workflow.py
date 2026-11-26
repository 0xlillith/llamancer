#!/usr/bin/env python3
"""Capture the actual isolated Kitty UI. Images rasterize terminal cells, not mockups.

Requires Kitty, Pillow, fontconfig, a graphical session, a CPU llama-server,
and local public GGUF fixtures. No user configuration or private projects.
"""
import json, os, re, socket, subprocess as sp, sys, tempfile, time, unicodedata
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

binary, backend, models, output = (Path(x).resolve() for x in sys.argv[1:])
output.mkdir(parents=True, exist_ok=True)
gui = dict(os.environ)
for line in sp.check_output(["systemctl","--user","show-environment"], text=True).splitlines():
    k,_,v=line.partition("=")
    if k in ("DISPLAY","WAYLAND_DISPLAY","XDG_RUNTIME_DIR","DBUS_SESSION_BUS_ADDRESS","XDG_SESSION_TYPE"):
        gui[k]=v
fontpath = sp.check_output(["fc-match","JetBrainsMono Nerd Font Mono","-f","%{file}"],text=True).strip()
font = ImageFont.truetype(fontpath, 17)
boldpath = sp.check_output(["fc-match","JetBrainsMono Nerd Font Mono:style=Bold","-f","%{file}"],text=True).strip()
boldfont = ImageFont.truetype(boldpath,17)
cw, ch = round(font.getlength("M")), 24
default_bg, default_fg = "#171624","#e7e3f5"
ansi16=["#171624","#f7768e","#a6e3b2","#f9e2af","#89b4fa","#b7a1f5","#8fe4ec","#e7e3f5","#686579","#ff9cac","#c4f3cb","#fbe8bd","#b2cdfd","#dac6fc","#b8f6f4","#ffffff"]
def rgb256(i):
    if i<16:return ansi16[i]
    if i<232:
        n=i-16; levels=[0,95,135,175,215,255]
        return tuple(levels[k] for k in (n//36,(n//6)%6,n%6))
    return (8+(i-232)*10,)*3
def render(raw,dest):
    # Kitty get-text emits SGR styling and literal terminal lines. Keep
    # terminal cell alignment, true colors, and wide glyph boundaries.
    lines=raw.splitlines()
    cols=148
    pad=12
    im=Image.new("RGB",(cols*cw+2*pad,len(lines)*ch+2*pad),default_bg)
    draw=ImageDraw.Draw(im)
    fg,bg,bold,reverse=default_fg,default_bg,False,False
    for y,line in enumerate(lines):
        x=0
        for token in re.findall(r"\x1b\[[0-?]*[ -/]*[@-~]|[^\x1b]+|\x1b",line):
            if token.startswith("\x1b"):
                if not token.endswith("m"):continue
                p=token[2:-1]
                parts=p.split(";") if p else ["0"]
                i=0
                while i<len(parts):
                    a=parts[i];i+=1
                    if ":" in a:
                        bits=a.split(":")
                        try:
                            code,mode=map(int,bits[:2])
                            color=tuple(map(int,bits[-3:])) if mode==2 else rgb256(int(bits[-1])) if mode==5 else None
                            if color is not None:
                                if code==38:fg=color
                                elif code==48:bg=color
                        except (ValueError,IndexError):pass
                        continue
                    try:n=int(a or "0")
                    except ValueError:continue
                    if n==0:fg,bg,bold,reverse=default_fg,default_bg,False,False
                    elif n==1:bold=True
                    elif n==22:bold=False
                    elif n==7:reverse=True
                    elif n==27:reverse=False
                    elif n==39:fg=default_fg
                    elif n==49:bg=default_bg
                    elif 30<=n<=37:fg=ansi16[n-30]
                    elif 40<=n<=47:bg=ansi16[n-40]
                    elif 90<=n<=97:fg=ansi16[n-90+8]
                    elif 100<=n<=107:bg=ansi16[n-100+8]
                    elif n in (38,48) and i<len(parts):
                        try:
                            mode=int(parts[i]);i+=1
                            if mode==2:color=tuple(map(int,parts[i:i+3]));i+=3
                            elif mode==5:color=rgb256(int(parts[i]));i+=1
                            else:continue
                            if n==38:fg=color
                            else:bg=color
                        except (ValueError,IndexError):pass
                continue
            for char in token:
                if unicodedata.combining(char) or char in "\ufe0f\u200d":continue
                if ord(char)<32:continue
                width=2 if unicodedata.east_asian_width(char) in ("W","F") else 1
                if x+width>cols:break
                xx,yy=pad+x*cw,pad+y*ch
                ink,paper=(bg,fg) if reverse else (fg,bg)
                draw.rectangle((xx,yy,xx+width*cw-1,yy+ch-1),fill=paper)
                draw.text((xx,yy+2),char,font=boldfont if bold else font,fill=ink)
                x+=width
    im.save(dest,optimize=True)

with tempfile.TemporaryDirectory(prefix="llamancer-gallery-") as scratch:
    root=Path(scratch); cfg=root/"config/llamancer"; cfg.mkdir(parents=True)
    home=root/"home";home.mkdir()
    workspace=root/"Projects";workspace.mkdir()
    for n in ["asteria","local-agent","rust-playground","web-dashboard"]:
        (workspace/n).mkdir()
    modelroot=root/"Models";modelroot.mkdir()
    fixtures=root/"fixtures";fixtures.mkdir()
    sources=sorted(models.glob("*.gguf"))
    if os.environ.get("LLAMANCER_CAPTURE_HYBRID"):
        sources.append(Path(os.environ["LLAMANCER_CAPTURE_HYBRID"]).resolve(strict=True))
    for source in sources:
        (fixtures/source.name).symlink_to(source)
    files=sorted(fixtures.glob("*.gguf"))
    with socket.socket() as s:s.bind(("127.0.0.1",0));port=s.getsockname()[1]
    endpoint=f"http://127.0.0.1:{port}"
    config={"theme":"andromeda","icon_mode":"nerd","model_imports":[str(p) for p in files],"model_roots":[str(modelroot)],"workspace_root":str(workspace),"project_dir":str(workspace/"asteria"),"server_binary":str(backend),"endpoint":endpoint,"device":"none","default_agent":"omp"}
    (cfg/"config.toml").write_text("\n".join(f"{k}={json.dumps(v)}" for k,v in config.items())+"\n")
    env={k:v for k,v in gui.items() if k in ("PATH","DISPLAY","WAYLAND_DISPLAY","XDG_RUNTIME_DIR","DBUS_SESSION_BUS_ADDRESS","XDG_SESSION_TYPE","LANG","LC_ALL")}
    env.update(HOME=str(home),XDG_CONFIG_HOME=str(cfg.parent),XDG_DATA_HOME=str(root/"data"),HF_HUB_DISABLE_IMPLICIT_TOKEN="1")
    remote="unix:"+str(root/"kitty.sock")
    app=sp.Popen(["kitty","--config","NONE","--override","allow_remote_control=socket-only","--listen-on",remote,"--override","initial_window_width=148c","--override","initial_window_height=36c","--override","remember_window_size=no","--override","font_size=12","--override","font_family=JetBrainsMono Nerd Font Mono","--override","window_padding_width=10","--title","Llamancer documentation capture",str(binary)],env=env,cwd=workspace,stdout=sp.DEVNULL,stderr=sp.DEVNULL)
    def rc(*args):return sp.check_output(["kitten","@","--to",remote,*args],env=gui,text=True,stderr=sp.DEVNULL,timeout=5)
    def wait(marker,timeout=30):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            if app.poll() is not None:raise RuntimeError("Kitty exited")
            try:
                data=rc("get-text")
                if marker in data:return data
            except (OSError,sp.SubprocessError):pass
            time.sleep(.15)
        raise RuntimeError(f"Missing {marker!r}: {rc('get-text')}")
    def text(v):rc("send-text",v);time.sleep(.2)
    def key(*v):rc("send-key",*v);time.sleep(.2)
    def shot(name,marker):
        wait(marker);time.sleep(.25)
        data=rc("get-text","--ansi")
        render(data,output/f"workflow-{name}.png")
        print("Captured",name,flush=True)
    try:
        wait("Scan complete")
        shot("library","Qwen")
        key("enter");shot("launch","Configuration preview")
        # Project selector accessible from Launch via explicit row navigation.
        key("down","down","down","down","down","down");key("enter")
        shot("projects","Project workspace")
        key("enter");wait("Launch model");key("esc");wait("Run model")
        key("2");wait("Search models");key("f3");text("@unsloth/Qwen3.8-Flash-Next-GGUF")
        wait("@unsloth/Qwen3.8-Flash-Next-GGUF");key("enter")
        wait("Variant / quantization",45)
        shot("discover","UD-")
        # Return to repository query and download a small public model.
        key("backspace");wait("Search models");key("f3");key("ctrl+u");text("@ggml-org/tiny-llamas");wait("@ggml-org/tiny-llamas");key("enter")
        wait("stories260K.gguf",45);key("f3");text("stories260K.gguf");key("enter")
        wait("1 variants");key("enter");wait("Download destination");key("enter")
        wait("Queue complete",60)
        shot("downloads","Transfer dashboard")
        key("1");wait("stories260K")
        key("f3");text("Qwen3.5-0.8B" if os.environ.get("LLAMANCER_CAPTURE_HYBRID") else "Qwen3-0.6B");key("esc");key("enter");key("down");key("enter")
        wait("Connected",60)
        shot("server","Actual context")
        key("x");wait("Managed server stopped")
        key("5");shot("settings","Download transport")
        key("q");app.wait(timeout=15)
    finally:
        if app.poll() is None:
            try:rc("send-key","ctrl+c");app.wait(timeout=15)
            except Exception:app.terminate();app.wait(timeout=10)
