"""Optional private-fixture intake smoke. Does not measure model recognition accuracy."""
import json,os,subprocess,tempfile,time,unittest,urllib.request
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
FIX=ROOT/'fixtures/private'
class RealImportSmoke(unittest.TestCase):
 @unittest.skipUnless(os.getenv('STUDY_REAL_FIXTURES')=='1','Private real PDF fixture test is opt-in')
 def test_real_pdf_sources_remain_unpublished_without_agent(self):
  papers=sorted(FIX.glob('*.pdf'));self.assertEqual(len(papers),4)
  with tempfile.TemporaryDirectory() as td:
   ready=Path(td)/'ready';env={**os.environ,'STUDY_ROOT':str(ROOT),'STUDY_DATA_DIR':td,'STUDY_PORT':'0','STUDY_READY_FILE':str(ready)}
   proc=subprocess.Popen([str(ROOT/'server/target/debug/study-core')],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
   try:
    for _ in range(100):
     if ready.exists():break
     time.sleep(.05)
    base=ready.read_text();body=b''
    for path in papers:
     body+=f'--REAL\r\nContent-Disposition: form-data; name="files"; filename="{path.name}"\r\nContent-Type: application/pdf\r\n\r\n'.encode()+path.read_bytes()+b'\r\n'
    body+=b'--REAL--\r\n'
    req=urllib.request.Request(base+'/api/imports',data=body,headers={'X-Study-Client':'private-fixture-test','Content-Type':'multipart/form-data; boundary=REAL'})
    with urllib.request.urlopen(req,timeout=60) as r:data=json.load(r)
    self.assertEqual(len(data['imports']),4);self.assertTrue(all(x['status']=='awaiting_agent' for x in data['imports']))
    self.assertEqual(data['recognition_job']['status'],'awaiting_agent');self.assertEqual(len(data['recognition_job']['sources']),4)
    self.assertEqual(sorted(x['pages'] for x in data['imports']),[4,13,16,19])
    with urllib.request.urlopen(base+'/api/questions') as r:self.assertEqual(json.load(r)['total'],0)
   finally:proc.terminate();proc.communicate(timeout=5)
if __name__=='__main__':unittest.main()
