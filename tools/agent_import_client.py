#!/usr/bin/env python3
"""Local adapter for any future Agent runner. Never selects/calls an AI provider.
Example: agent_import_client.py --job ID prepare --dest /tmp/study-agent-input
"""
import argparse,hashlib,json,sys,urllib.request,urllib.parse
from pathlib import Path

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--base',default='http://127.0.0.1:4317');p.add_argument('--job',required=True)
 sub=p.add_subparsers(dest='command',required=True)
 prep=sub.add_parser('prepare');prep.add_argument('--dest',required=True)
 cl=sub.add_parser('claim');cl.add_argument('--role',choices=['extractor','verifier'],required=True);cl.add_argument('--run-id',required=True);cl.add_argument('--name',default='external-agent')
 prop=sub.add_parser('submit');prop.add_argument('--proposal',required=True)
 ver=sub.add_parser('verify');ver.add_argument('--verification',required=True)
 a=p.parse_args();base=urllib.parse.urlparse(a.base)
 if base.scheme!='http' or base.hostname not in ('127.0.0.1','localhost') or base.username or base.password or base.path not in ('','/'):
  p.error('Only a loopback local HTTP base is supported. No external data destination is configured.')
 def request(path,body=None):
  raw=json.dumps(body).encode() if body is not None else None
  req=urllib.request.Request(a.base.rstrip('/')+path,data=raw,headers={'X-Study-Client':'local-agent-adapter','Content-Type':'application/json'})
  with urllib.request.urlopen(req,timeout=120) as r:return r.read()
 route='/api/recognition/jobs/'+urllib.parse.quote(a.job,safe='')
 if a.command=='prepare':
  manifest=json.loads(request(route+'/input'));dest=Path(a.dest).resolve();dest.mkdir(parents=True,exist_ok=True)
  for source in manifest['sources']:
   data=request(source['file_url']);digest=hashlib.sha256(data).hexdigest()
   if digest!=source['sha256']:raise RuntimeError('Downloaded PDF does not match its manifest hash')
   sid=source['id']
   if '/' in sid or '\\' in sid or '..' in sid:raise ValueError('Invalid source id')
   path=dest/(sid+'.pdf')
   if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest()!=digest:raise RuntimeError('Refusing to overwrite different local source')
   path.write_bytes(data);source['local_path']=str(path)
  target=dest/'job-input.json';target.write_text(json.dumps(manifest,ensure_ascii=False,indent=2));print(json.dumps({'manifest':str(target),'files':len(manifest['sources']),'provider_called':False}))
 elif a.command=='claim':print(request(route+'/claim',{'agent_run_id':a.run_id,'agent_name':a.name,'role':a.role}).decode())
 else:
  file=Path(a.proposal if a.command=='submit' else a.verification)
  if file.stat().st_size>4*1024*1024:raise ValueError('Result exceeds 4 MiB')
  print(request(route+('/proposal' if a.command=='submit' else '/verification'),json.loads(file.read_text())).decode())
if __name__=='__main__':
 try:main()
 except Exception as e:print(str(e),file=sys.stderr);sys.exit(1)
