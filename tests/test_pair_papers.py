import sys, json, hashlib, tempfile, unittest, copy
from pathlib import Path
import pymupdf as fitz
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from pair_papers import validate, schema_only, OMITTED
class PairTests(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
  self.m={'schema_version':'1','sources':[]}
  for role in ['qp','ms']:
   p=Path(self.tmp.name)/(role+'.pdf'); d=fitz.open();page=d.new_page();page.insert_text((60,80),'1 Explain acceleration.');d.save(p);d.close()
   self.m['sources'].append({'id':role,'role':role,'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'pages':1,'verified_exam_key':'9702/22/s/2024'})
  def span(s):return {'source_id':s,'page':1,'bbox':[50,50,500,100],'quote':'1 Explain acceleration.'}
  self.q={'stem_text':'1 Explain acceleration.','answer_text':'1 Explain acceleration.','exam_key':'9702/22/s/2024','question_no':'1','subparts':[],'question':[span('qp')],'answer':[span('ms')],'figures':[],'confidence':.99,'exception_flags':[]}
  self.p={'schema_version':'1','agent_run_id':'fixture','questions':[self.q]}
 def attest(self):
  self.m['verified_questions']=[{'exam_key':self.q['exam_key'],'question_no':'1','proposal_sha256':hashlib.sha256(json.dumps(self.q,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest(),'checks':['boundaries_complete','answer_pair_correct','figures_complete','subparts_complete','transcription_complete']}]
 def test_self_confidence_not_accepted(self):
  r=validate(self.m,self.p);self.assertEqual(len(r['auto_ready']),0);self.assertIn('independent_verification_required',r['exception_queue'][0]['exception_flags'])
 def test_verified_accepted(self):
  self.attest();self.assertEqual(len(validate(self.m,self.p)['auto_ready']),1)
 def test_fabricated_quote(self):
  self.q['answer'][0]['quote']='fabrication';self.attest();self.assertIn('unverified_source_quote',validate(self.m,self.p)['exception_queue'][0]['exception_flags'])
 def test_bad_page(self):
  self.q['question'][0]['page']=2
  with self.assertRaises(ValueError):validate(self.m,self.p)
 def test_bad_box(self):
  self.q['question'][0]['bbox']=[-2,10,50,50]
  with self.assertRaises(ValueError):validate(self.m,self.p)
 def test_hash_changed(self):
  self.m['sources'][0]['sha256']='bad'
  with self.assertRaises(ValueError):validate(self.m,self.p)
 def test_identity_unverified(self):
  del self.m['sources'][0]['verified_exam_key'];self.attest();self.assertIn('exam_identity_unverified',validate(self.m,self.p)['exception_queue'][0]['exception_flags'])
 def test_wrong_role(self):
  self.q['answer'][0]['source_id']='qp'
  with self.assertRaises(ValueError):validate(self.m,self.p)
 def test_duplicate(self):
  self.p['questions'].append(copy.deepcopy(self.q))
  with self.assertRaises(ValueError):validate(self.m,self.p)
 def test_missing_answer(self):
  self.q['answer']=[];self.attest();self.assertIn('missing_answer',validate(self.m,self.p)['exception_queue'][0]['exception_flags'])
 def test_same_answer_region_cannot_be_reused(self):
  other=copy.deepcopy(self.q);other['question_no']='2';self.p['questions'].append(other)
  with self.assertRaises(ValueError):validate(self.m,self.p)
 def test_stale_acceptance_not_accepted(self):
  self.attest();self.q['subparts']=['a'];self.assertFalse(validate(self.m,self.p)['auto_ready'])
 def test_nan_not_accepted(self):
  self.q['question'][0]['bbox'][0]=float('nan')
  with self.assertRaises(ValueError):validate(self.m,self.p)
 def test_metrics_dont_claim_ai_accuracy(self):
  r=validate(self.m,self.p);self.assertIsNone(r['metrics']['ai_accuracy']);self.assertEqual(r['metrics']['benchmark_status'],'not_evaluated')
 def test_schema_only_digest(self):
  r=schema_only(self.p);self.attest();self.assertTrue(r['valid']);self.assertEqual(r['question_digests'][0]['proposal_sha256'],self.m['verified_questions'][0]['proposal_sha256'])
 def test_schema_only_cli(self):
  import subprocess
  p=Path(self.tmp.name)/'proposal.json';p.write_text(json.dumps(self.p))
  r=subprocess.run([sys.executable,str(Path(__file__).resolve().parents[1]/'tools/pair_papers.py'),'--schema-only',str(p)],capture_output=True,text=True)
  self.assertEqual(r.returncode,0);self.assertTrue(json.loads(r.stdout)['valid'])
 def test_copyright_detection(self):
  self.assertTrue(OMITTED.search('Content removed due to copyright restrictions.'))
  self.assertFalse(OMITTED.search('Copyright Cambridge 2025. Permission to reproduce items has been sought.'))
 def test_multiple_qp_files_exception(self):
  source=dict(self.m['sources'][0],id='qp2');self.m['sources'].append(source)
  self.q['question'].append(dict(self.q['question'][0],source_id='qp2'));self.attest()
  self.assertIn('multiple_qp_sources',validate(self.m,self.p)['exception_queue'][0]['exception_flags'])
 def make_visual(self):
  from render_pdf_page import render_region, RENDER_VERSION
  source=self.m['sources'][0];p=Path(source['path']);d=fitz.open(p);pix=d[0].get_pixmap();d.close()
  d=fitz.open();page=d.new_page();page.insert_image(page.rect,stream=pix.tobytes('png'));data=d.tobytes();d.close();p.write_bytes(data)
  source['sha256']=hashlib.sha256(data).hexdigest();d=fitz.open(p);span=self.q['question'][0]
  v={'source_id':'qp','page':1,'bbox':span['bbox'],'quote_sha256':hashlib.sha256(span['quote'].encode()).hexdigest(),'image_sha256':hashlib.sha256(render_region(d[0],span['bbox'])).hexdigest(),'render_version':RENDER_VERSION,'verifier_run_id':'independent-run','checks':['transcription_correct','region_complete']};d.close()
  self.m['visual_verifications']=[v];self.attest();return v
 def test_verified_visual_scan_can_pass(self):
  self.make_visual();r=validate(self.m,self.p);self.assertEqual(len(r['auto_ready']),1);self.assertEqual(r['auto_ready'][0]['evidence'][0]['verification_method'],'independent_visual')
 def test_visual_wrong_image_fails(self):
  v=self.make_visual();v['image_sha256']='tampered';self.assertFalse(validate(self.m,self.p)['auto_ready'])
 def test_visual_extractor_cannot_verify_itself(self):
  v=self.make_visual();v['verifier_run_id']='fixture';self.assertFalse(validate(self.m,self.p)['auto_ready'])
 def test_visual_changed_quote_fails(self):
  self.make_visual();self.q['question'][0]['quote']='Changed';self.attest();self.assertFalse(validate(self.m,self.p)['auto_ready'])
 def test_incomplete_transcription_acceptance_exception(self):
  self.attest();self.m['verified_questions'][0]['checks'].remove('transcription_complete')
  self.assertIn('independent_verification_required',validate(self.m,self.p)['exception_queue'][0]['exception_flags'])
 def test_empty_answer_content_exception(self):
  self.q['answer_text']='';self.attest()
  self.assertIn('missing_answer_text',validate(self.m,self.p)['exception_queue'][0]['exception_flags'])
 def test_extra_source_in_figures_exception(self):
  self.m['sources'].append(dict(self.m['sources'][0],id='qp-extra'))
  self.q['figures']=[dict(self.q['question'][0],source_id='qp-extra')];self.attest()
  self.assertIn('multiple_qp_sources',validate(self.m,self.p)['exception_queue'][0]['exception_flags'])
if __name__=='__main__':unittest.main()
