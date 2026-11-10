"""Protocol integration with synthetic agent outputs, not a model accuracy test."""
import io,json,os,subprocess,tempfile,time,unittest,urllib.request,urllib.error,hashlib
from pathlib import Path
import pymupdf as fitz
ROOT=Path(__file__).resolve().parents[1]
class RecognitionFlow(unittest.TestCase):
 def test_extract_independent_verify_auto_publish(self):
  with tempfile.TemporaryDirectory() as td:
   ready=Path(td)/'ready';env={**os.environ,'STUDY_ROOT':str(ROOT),'STUDY_DATA_DIR':td,'STUDY_PORT':'0','STUDY_READY_FILE':str(ready)}
   proc=subprocess.Popen([str(ROOT/'server/target/debug/study-core')],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
   try:
    for _ in range(100):
     if ready.exists():break
     time.sleep(.05)
    base=ready.read_text()
    def req(path,body=None,content='application/json'):
     raw=body if isinstance(body,bytes) else json.dumps(body).encode() if body is not None else None
     r=urllib.request.Request(base+path,data=raw,headers={'Content-Type':content,'X-Study-Client':'synthetic-test'})
     with urllib.request.urlopen(r,timeout=30) as response:return json.load(response)
    def upload(role,text):
     doc=fitz.open();p=doc.new_page(width=595,height=842);p.insert_text((72,72),text);data=doc.tobytes();doc.close()
     body=f'--B\r\nContent-Disposition: form-data; name="files"; filename="9702_m25_{role}_22.pdf"\r\nContent-Type: application/pdf\r\n\r\n'.encode()+data+b'\r\n--B--\r\n'
     result=req('/api/imports',body,'multipart/form-data; boundary=B');self.assertEqual(result['recognition_job']['status'],'awaiting_agent');return result['imports'][0]['id']
    qp=upload('qp','1 Calculate current.');ms=upload('ms','1 Current is 1 ampere.')
    job=req('/api/recognition/jobs',{'import_ids':[qp,ms]});route='/api/recognition/jobs/'+job['id']
    self.assertEqual(req('/api/recognition/jobs',{'import_ids':[ms,qp]})['id'],job['id'])
    self.assertFalse(req('/api/recognition/status')['provider_configured'])
    req(route+'/claim',{'agent_run_id':'extract-1','role':'extractor'})
    with self.assertRaises(urllib.error.HTTPError) as e:req(route+'/claim',{'agent_run_id':'extract-2','role':'extractor'})
    self.assertEqual(e.exception.code,409)
    span=lambda sid,quote:{'source_id':sid,'page':1,'bbox':[0.00001,0.00001,595.0,842.0],'quote':quote}
    q={'exam_key':'9702/22/m/2025','question_no':'1','subparts':[],'stem_text':'1 Calculate current.','answer_text':'1 Current is 1 ampere.','question':[span(qp,'1 Calculate current.')],'answer':[span(ms,'1 Current is 1 ampere.')],'figures':[],'confidence':.99,'exception_flags':[]}
    image=req(route+'/region',{'source_id':qp,'page':1,'bbox':[0,0,595,842]});self.assertEqual(len(image['image_sha256']),64);self.assertEqual(image['render_version'],'pdfcraft-region-v2')
    job=req(route+'/proposal',{'schema_version':'1','agent_run_id':'extract-1','questions':[q]})
    self.assertEqual(job['status'],'awaiting_verification');self.assertEqual(req('/api/questions')['total'],0)
    self.assertEqual(req(route+'/input')['question_digests'][0]['proposal_sha256'],hashlib.sha256(json.dumps(q,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest())
    with self.assertRaises(urllib.error.HTTPError) as e:req(route+'/claim',{'agent_run_id':'extract-1','role':'verifier'})
    self.assertEqual(e.exception.code,400)
    req(route+'/claim',{'agent_run_id':'verify-1','role':'verifier'})
    review={'agent_run_id':'verify-1','source_verifications':[{'source_id':qp,'role':'qp','verified_exam_key':q['exam_key']},{'source_id':ms,'role':'ms','verified_exam_key':q['exam_key']}],'verified_questions':[{**job['question_digests'][0],'checks':['boundaries_complete','answer_pair_correct','figures_complete','subparts_complete','transcription_complete']}]}
    complete=req(route+'/verification',review)
    self.assertEqual(complete['status'],'completed');self.assertEqual(complete['published_count'],1)
    self.assertIsNone(complete['metrics']['ai_accuracy']);self.assertEqual(complete['metrics']['benchmark_status'],'not_evaluated')
    self.assertEqual(req('/api/questions')['questions'][0]['status'],'agent_verified')
    self.assertTrue(req('/api/questions')['questions'][0]['source_regions']['question'])
    with self.assertRaises(urllib.error.HTTPError) as e:req(route+'/verification',review)
    self.assertEqual(e.exception.code,409)
   finally:proc.terminate();proc.communicate(timeout=5)
if __name__=='__main__':unittest.main()
