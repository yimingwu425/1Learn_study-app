"""Real Rust HTTP + SQLite + PDF end-to-end smoke, isolated temporary database."""
import io,json,os,socket,subprocess,tempfile,time,unittest,urllib.request,urllib.error,zipfile
from pathlib import Path
import pymupdf as fitz
ROOT=Path(__file__).resolve().parents[1]
BIN=ROOT/'server/target/debug/study-core'
class HttpFlow(unittest.TestCase):
 def test_import_review_study_export(self):
  if not BIN.exists():self.skipTest('Build Rust binary first')
  with tempfile.TemporaryDirectory() as td:
   with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
   env={**os.environ,'STUDY_DATA_DIR':td,'STUDY_PORT':str(port),'STUDY_ROOT':str(ROOT)}
   proc=subprocess.Popen([str(BIN)],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
   base=f'http://127.0.0.1:{port}'
   def req(path,data=None,content='application/json',headers=None,method=None):
    h={'X-Study-Client':'integration-test',**(headers or {})}
    if data is not None:h['Content-Type']=content
    raw=data if isinstance(data,bytes) else (json.dumps(data).encode() if data is not None else None)
    try:
     with urllib.request.urlopen(urllib.request.Request(base+path,data=raw,headers=h,method=method),timeout=10) as r:return r.read()
    except urllib.error.HTTPError as e:
     e.detail=e.read().decode();print('HTTP error',path,e.code,e.detail);raise
   def upload(name,text):
    doc=fitz.open();page=doc.new_page();page.insert_text((72,72),text);b=doc.tobytes();doc.close()
    body=b'--boundary\r\nContent-Disposition: form-data; name="files"; filename="'+name.encode()+b'"\r\nContent-Type: application/pdf\r\n\r\n'+b+b'\r\n--boundary--\r\n'
    return json.loads(req('/api/imports',body,'multipart/form-data; boundary=boundary'))['imports'][0]
   try:
    for _ in range(50):
     try:req('/api/health');break
     except Exception:time.sleep(.1)
    qp=upload('9702_m25_qp_22.pdf','1 Calculate the current. [2]')
    ms=upload('9702_m25_ms_22.pdf','1 Current = 1.0 A [2]')
    self.assertEqual(qp['status'],'awaiting_agent');self.assertEqual(ms['role'],'ms');self.assertEqual(qp['inspection_engine'],'Rust + PdfCraft 0.20.0')
    # PyMuPDF document IDs vary, so duplicate test explicitly retransmits the saved file.
    content=Path(td,'files',qp['id']+'.pdf').read_bytes()
    body=b'--boundary\r\nContent-Disposition: form-data; name="files"; filename="9702_m25_qp_22.pdf"\r\n\r\n'+content+b'\r\n--boundary--\r\n'
    same=json.loads(req('/api/imports',body,'multipart/form-data; boundary=boundary'))['imports'][0]
    self.assertTrue(same['duplicate']);self.assertEqual(same['id'],qp['id'])
    self.assertEqual(json.loads(req('/api/questions'))['total'],0)
    candidate={'question_no':'1','qp_pages':[1],'ms_pages':[1],'text':'Calculate the current.','marks':2,'topics':['内阻']}
    with self.assertRaises(urllib.error.HTTPError) as e:req('/api/imports/'+qp['id']+'/publish',{'candidates':[{**candidate,'qp_pages':[9]}],'ms_import_id':ms['id']})
    self.assertEqual(e.exception.code,400)
    published=json.loads(req('/api/imports/'+qp['id']+'/publish',{'candidates':[candidate],'ms_import_id':ms['id']}))
    self.assertEqual(published['published'],1);qid=published['questions'][0]['id']
    self.assertEqual(json.loads(req('/api/questions?subject=9702'))['total'],1)
    attempt=json.loads(req('/api/attempts',{'question_id':qid,'status':'hinted','note':'Remember internal resistance.'}))
    self.assertEqual(attempt['status'],'hinted');self.assertEqual(len(json.loads(req('/api/attempts'))['attempts']),1)
    with self.assertRaises(urllib.error.HTTPError) as e:req('/api/attempts',{'question_id':qid,'status':'ai_correct'})
    self.assertEqual(e.exception.code,400)
    exp=json.loads(req('/api/exports',{'question_ids':[qid],'mode':'practice'}))
    with zipfile.ZipFile(io.BytesIO(req(exp['url']))) as z:self.assertIn('student.pdf',z.namelist());self.assertIn('answers.pdf',z.namelist());self.assertIn('provenance.json',z.namelist())
    selected=json.loads(req('/api/basket/export',{'subject':'9702','ids':[qid]}))
    with zipfile.ZipFile(io.BytesIO(req(selected['url']))) as z:self.assertIn('student.pdf',z.namelist())
    self.assertEqual(selected['format'],'complete_source_pages')
    with self.assertRaises(urllib.error.HTTPError) as e:req('/api/attempts',{'question_id':qid,'status':'retry'},headers={'Origin':'https://evil.example'})
    self.assertEqual(e.exception.code,403)
    with self.assertRaises(urllib.error.HTTPError) as e:req('/api/imports/'+qp['id']+'/publish',{'candidates':[candidate],'ms_import_id':ms['id']})
    self.assertEqual(e.exception.code,409)
    agent=json.loads(req('/api/agent/tools',{'name':'search_questions','args':{'subject':'9702','keyword':'current'}}))
    self.assertEqual(agent['total'],1)
    self.assertEqual(json.loads(req('/api/agent/tools',{'tool':'search_questions','arguments':{'subject':'9702','yearFrom':2025}}))['total'],1)
    self.assertEqual(json.loads(req('/api/agent/tools',{'tool':'search_questions','arguments':{'subject':'9702','yearFrom':2026}}))['total'],0)
    self.assertEqual(json.loads(req('/api/questions?subject=9702&offset=1&limit=1'))['questions'],[])
    self.assertEqual(json.loads(req('/api/questions?subject=9702&offset=1&limit=1'))['total'],1)
    unknown=upload('my-answers.pdf','1 F = ma [2]')
    fixed=json.loads(req('/api/imports/'+unknown['id'],{'subject':'9702','exam_code':'9702_w25_22','role':'ms'},method='PATCH'))
    self.assertEqual(fixed['role'],'ms');self.assertEqual(fixed['exam_code'],'9702_w25_22')
    with self.assertRaises(urllib.error.HTTPError) as e:req('/api/imports/'+qp['id'],{'subject':'9702','exam_code':'9702_m25_22','role':'ms'},method='PATCH')
    self.assertEqual(e.exception.code,409)
    study=json.loads(req('/api/exports',{'question_ids':[qid],'mode':'study'}))
    with zipfile.ZipFile(io.BytesIO(req(study['url']))) as z:
     with fitz.open(stream=z.read('student.pdf'),filetype='pdf') as pdf:self.assertIn('Remember internal resistance.',pdf[0].get_text())
   finally:proc.terminate();proc.communicate(timeout=5)
if __name__=='__main__':unittest.main()
