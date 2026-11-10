"""The retained Agent envelopes execute native rules with the compatibility service unavailable."""
import json,os,sqlite3,subprocess,tempfile,time,unittest,urllib.request,urllib.error
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class NativeGatewayTest(unittest.TestCase):
 def test_native_contract_and_both_envelopes(self):
  with tempfile.TemporaryDirectory() as td:
   ready=Path(td)/'ready';env={**os.environ,'STUDY_ROOT':str(ROOT),'STUDY_DATA_DIR':td,'STUDY_PORT':'0','STUDY_READY_FILE':str(ready),'STUDY_LEGACY_AGENT_URL':'http://127.0.0.1:1/api/agent/tools'}
   proc=subprocess.Popen([str(ROOT/'server/target/debug/study-core')],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
   try:
    for _ in range(100):
     if ready.exists():break
     time.sleep(.05)
    base=ready.read_text()+'/api/agent/tools'
    catalog=json.load(urllib.request.urlopen(base));self.assertEqual(catalog['legacy_contract'],'2.2.0');self.assertEqual(len(catalog['tools']),8);self.assertTrue(all(t['implementation']=='rust' for t in catalog['tools']))
    with sqlite3.connect(Path(td)/'study.sqlite') as db:
     for i in range(2):
      q={'id':f'test:{i}','subject':'9702','source':'9702_m25_12','year':2025,'paper_type':'paper1','question_no':str(i+1),'marks':1,'text':'unverified full-page content','kind':'real','legacy_metadata':{'question':{'question_id':900000+i,'question_order':i,'confidence':'high','needs_review':0},'paper':{'season':'m','component':'12','variant':'2','source_type':'past_paper'},'tags':[{'tag_id':'FIXTURE','tag_role':'primary','syllabus_ref':'1.1.1','topic':'1 Units','subtopic':'1.1 Units','weighted_marks_for_tag':1}]}}
      db.execute('INSERT INTO questions VALUES(?,?,?)',(q['id'],'9702',json.dumps(q)))
    def call(body):return urllib.request.urlopen(urllib.request.Request(base,data=json.dumps(body).encode(),headers={'Content-Type':'application/json','X-Study-Client':'test'}),timeout=20)
    a=json.load(call({'name':'search_questions','args':{'subject':'9702','yearFrom':2025}}));b=json.load(call({'tool':'search_questions','arguments':{'subject':'9702','yearFrom':2025}}));self.assertEqual(a,b);self.assertEqual(a['total'],2)
    plan=json.load(call({'tool':'generate_exam_paper','arguments':{'subject':'9702','paperType':'paper1','mode':'assignment','targetMarks':2}}));self.assertEqual(plan['selectionEngine'],'rust');self.assertEqual(plan['actualMarks'],2);self.assertEqual(plan['actualQuestions'],2)
    with self.assertRaises(urllib.error.HTTPError) as e:call({'tool':'search_questions','arguments':{'subject':'9702','unknownFilter':'silently-ignore'}})
    self.assertEqual(e.exception.code,400)
    with self.assertRaises(urllib.error.HTTPError) as e:call({'tool':'get_subjects','arguments':[]})
    self.assertEqual(e.exception.code,400)
   finally:proc.terminate();proc.communicate(timeout=5)
if __name__=='__main__':unittest.main()
