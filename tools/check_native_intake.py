"""Real PDF intake against an isolated database; no model is called or question published."""
import json,os,subprocess,tempfile,time,urllib.request,hashlib
from pathlib import Path
import pymupdf
ROOT=Path(__file__).resolve().parents[1]
sources=[Path('/Users/yimingwu/Downloads/AS Phys 9702/2023/QP/9702_w23_qp_12.pdf'),Path('/Users/yimingwu/Downloads/AS Phys 9702/2023/MS/9702_w23_ms_12.pdf')]
op=urllib.request.build_opener(urllib.request.ProxyHandler({}));report={'sources':[]}
with tempfile.TemporaryDirectory() as td:
 ready=Path(td)/'ready';env={**os.environ,'STUDY_ROOT':str(ROOT),'STUDY_DATA_DIR':td,'STUDY_PORT':'0','STUDY_READY_FILE':str(ready)}
 proc=subprocess.Popen([str(ROOT/'server/target/debug/study-core')],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 try:
  for _ in range(100):
   if ready.exists():break
   time.sleep(.05)
  base=ready.read_text()
  def get(path):
   with op.open(base+path,timeout=120) as r:return json.load(r)
  body=b''
  for path in sources:
   raw=path.read_bytes();doc=pymupdf.open(path);report['sources'].append({'file':str(path),'sha256':hashlib.sha256(raw).hexdigest(),'expectedPages':len(doc)});doc.close()
   body+=f'--REALNATIVE\r\nContent-Disposition: form-data; name="files"; filename="{path.name}"\r\nContent-Type: application/pdf\r\n\r\n'.encode()+raw+b'\r\n'
  body+=b'--REALNATIVE--\r\n'
  request=urllib.request.Request(base+'/api/imports',data=body,headers={'Content-Type':'multipart/form-data; boundary=REALNATIVE','X-Study-Client':'real-native-intake'})
  with op.open(request,timeout=120) as r:result=json.load(r)
  assert len(result['imports'])==2
  for record,source in zip(result['imports'],report['sources']):
   assert record['status']=='awaiting_agent' and record['pages']==source['expectedPages'] and record['inspection_engine']=='Rust + PdfCraft 0.20.0'
   assert hashlib.sha256(Path(source['file']).read_bytes()).hexdigest()==source['sha256']
   source['inspectionEngine']=record['inspection_engine'];source['sourceUnchanged']=True
  assert get('/api/questions')['total']==0
  status=get('/api/recognition/status');assert not status['provider_configured']
  report.update(passed=True,publishedQuestions=0,jobStatus=result['recognition_job']['status'],recognitionModelConnected=False,usesIsolatedDatabase=True)
 finally:proc.terminate();proc.communicate(timeout=5)
(ROOT/'verification/native-real-import.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));print('PASS: real QP/MS, Rust + PdfCraft inspection, unchanged originals, zero unverified publications')
