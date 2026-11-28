#!/usr/bin/env python3
"""Opt-in diagnostic for the original Flash Next tool-call report.

Only calls a loopback server. No tools are executed. A successful synthetic
fixture is not certification of agent fidelity. Run with verified weights and
an explicitly chosen memory budget; this script never starts a server.
"""
import json
import sys
import time
import urllib.request
import urllib.error
from urllib.parse import urlsplit

base=sys.argv[1].rstrip("/")
url=urlsplit(base)
assert url.scheme=="http" and url.hostname in ("127.0.0.1","localhost","::1") and not url.username
def request(path,body=None):
    req=urllib.request.Request(base+path, data=None if body is None else json.dumps(body).encode(), headers={"Content-Type":"application/json"})
    with urllib.request.urlopen(req,timeout=150) as r:
        data=r.read(4*1024*1024+1)
    if len(data)>4*1024*1024:raise ValueError("Response too large")
    return data

model=json.loads(request("/v1/models"))["data"][0]["id"]
props=json.loads(request("/props"))
params=props.get("default_generation_settings",{}).get("params",{})
print("SERVER",json.dumps({"model":model,"context":props.get("default_generation_settings",{}).get("n_ctx"),"sampler":{k:params.get(k) for k in ("temperature","top_p","top_k","min_p","repeat_penalty","presence_penalty")},"caps":props.get("chat_template_caps")}),flush=True)
expected={"command":"git log --oneline -8","cwd":"/tmp/llamancer-fixture"}
prompt='Use shell exactly once with command "git log --oneline -8" and cwd "/tmp/llamancer-fixture". Do not modify any text in the arguments.'
system="You are a precise coding agent. Use a tool when asked."
functions=[{"name":"shell","description":"Run shell command in working directory","parameters":{"type":"object","properties":{"command":{"type":"string"},"cwd":{"type":"string"}},"required":["command","cwd"],"additionalProperties":False}},{"name":"read_file","description":"Read a file","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}]
def pairs(pairs):
    obj={}
    for k,v in pairs:
        if k in obj:raise ValueError("Duplicate argument key")
        obj[k]=v
    return obj
def check(name,args):
    return name=="shell" and json.loads(args,object_pairs_hook=pairs)==expected
def chat(thinking,greedy):
    body={"model":model,"messages":[{"role":"system","content":system},{"role":"user","content":prompt}],"tools":[{"type":"function","function":f} for f in functions],"chat_template_kwargs":{"enable_thinking":thinking},"max_tokens":256 if greedy else 2048,"stream":False}
    if greedy:body.update(temperature=0,seed=42)
    data=json.loads(request("/v1/chat/completions",body));choice=data["choices"][0];calls=choice["message"].get("tool_calls") or []
    valid=choice.get("finish_reason")=="tool_calls" and len(calls)==1 and check(calls[0]["function"]["name"],calls[0]["function"]["arguments"])
    return valid,{"finish":choice.get("finish_reason"),"calls":len(calls),"usage":data.get("usage")}
def responses():
    body={"model":model,"instructions":system,"input":[{"role":"user","content":prompt}],"tools":[dict(f,type="function") for f in functions],"reasoning":{"effort":"medium"},"max_output_tokens":2048,"stream":True}
    delta=[];done=None;completed=None;events=[];item_done=[]
    for line in request("/v1/responses",body).decode().splitlines():
        if not line.startswith("data:"):continue
        v=line[5:].strip()
        if v=="[DONE]":continue
        event=json.loads(v);kind=event.get("type");events.append(kind)
        if kind=="response.output_item.done":item_done.append({"index":event.get("output_index"),"item":{k:event.get("item",{}).get(k) for k in ("type","id","call_id","name","arguments")}})
        if kind=="response.function_call_arguments.delta":delta.append(event["delta"])
        elif kind=="response.function_call_arguments.done":done=event["arguments"]
        elif kind=="response.completed":completed=event["response"]
        elif kind in ("response.failed","response.incomplete","error"):return False,{"finish":kind}
    calls=[f for f in (completed or {}).get("output",[]) if f.get("type")=="function_call"]
    item_confirmations=[v for v in item_done if v["item"]["type"]=="function_call"]
    final_index=next((i for i,v in enumerate((completed or {}).get("output",[])) if v.get("type")=="function_call"),None)
    item_confirmed=len(calls)==1 and len(item_confirmations)==1 and item_confirmations[0]["index"]==final_index and all(item_confirmations[0]["item"].get(k)==calls[0].get(k) for k in ("id","call_id","name","arguments"))
    valid=(completed or {}).get("status")=="completed" and len(calls)==1 and "".join(delta)==calls[0]["arguments"] and (done==calls[0]["arguments"] or (done is None and item_confirmed)) and (not item_confirmations or item_confirmed) and check(calls[0]["name"],calls[0]["arguments"])
    detail={"finish":(completed or {}).get("status"),"calls":len(calls),"usage":(completed or {}).get("usage")}
    if len(calls)==1:
        detail.update(name=calls[0].get("name"),final_matches=check(calls[0]["name"],calls[0]["arguments"]),stream_matches_final="".join(delta)==calls[0]["arguments"],done_matches_final=done==calls[0]["arguments"],item_done_matches_final=item_confirmed,confirmation="arguments_done" if done is not None else "output_item_done" if item_confirmed else "missing")
        if not valid:
            detail.update(stream="".join(delta)[:512],done=done,final=calls[0]["arguments"][:512],event_types=sorted(set(events)),output_item_done=item_done)
    return valid,detail
for name,run in [("chat_recommended_thinking",lambda:chat(True,False)),("chat_original_nonthinking_greedy",lambda:chat(False,True)),("responses_medium",responses)]:
    start=time.monotonic()
    try:
        ok,detail=run()
        print("PROBE",name,"PASS" if ok else "FAIL",round(time.monotonic()-start,2),json.dumps(detail),flush=True)
    except Exception as e:
        print("PROBE",name,"ERROR",type(e).__name__,str(e)[:300],flush=True)
