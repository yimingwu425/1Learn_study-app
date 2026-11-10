"""Real immutable-plan edits, source export and economics choice-structure enforcement."""
import json,urllib.request,urllib.error,hashlib,zipfile,io,os
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];DATA=Path(os.getenv('STUDY_CHECK_DATA',ROOT/'data'));DEST=Path(os.getenv('STUDY_CHECK_DEST',ROOT/'verification/native-composition'));op=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def call(path,body=None,method=None):
 request=urllib.request.Request(os.getenv('STUDY_CHECK_BASE','http://127.0.0.1:4320')+path,data=None if body is None else json.dumps(body).encode(),method=method,headers={'Content-Type':'application/json','X-Study-Client':'immutable-plan-test'})
 with op.open(request,timeout=120) as r:return json.load(r)
report={}
original=call('/api/compositions',{'subject':'9702','paperType':'paper1','mode':'assignment','targetMarks':3})
path=DATA/'compositions'/f"{original['id']}.json";digest=hashlib.sha256(path.read_bytes()).hexdigest()
candidate=next(q for q in original['questions'][0]['replacementCandidates'] if q.get('id'))
changed=call('/api/compositions/'+original['id'],{'action':'replace','index':0,'replacementId':candidate['id']},'PATCH')
assert changed['id']!=original['id'] and changed['actualMarks']==3 and changed['exactTarget'] and changed['questions'][0]['id']==candidate['id']
assert hashlib.sha256(path.read_bytes()).hexdigest()==digest
reduced=call('/api/compositions/'+changed['id'],{'action':'remove','index':1},'PATCH')
assert reduced['actualMarks']==2 and reduced['actualQuestions']==2 and not reduced['exactTarget']
saved=call('/api/compositions/'+reduced['id']+'/export',{},'POST');saved_path=Path(saved['path']);assert saved_path.is_file()
with zipfile.ZipFile(saved_path) as z:
 manifest=json.loads(z.read(next(n for n in z.namelist() if n.endswith('manifest.json'))));assert manifest['actualMarks']==2
report['physics']={'original':original['id'],'replaced':changed['id'],'reduced':reduced['id'],'originalUnchanged':True,'replacementSameMarks':True,'localExport':saved}
try:call('/api/compositions/'+original['id'],{'action':'replace','index':0,'replacementId':original['questions'][1]['id']},'PATCH');raise AssertionError('unlisted replacement accepted')
except urllib.error.HTTPError as e:assert e.code==400
essay=next(x for x in json.loads((DEST/'report.json').read_text()) if x['name']=='economics-essays')['summary']
incomplete=call('/api/compositions/'+essay['id'],{'action':'remove','index':0},'PATCH')
assert incomplete['actualMarks']==40 and incomplete['printedMarks']==60 and not incomplete['exactTarget']
try:call('/api/compositions/'+incomplete['id']+'/export',{},'POST');raise AssertionError('incomplete exam exported')
except urllib.error.HTTPError as e:assert e.code==422
report['economics']={'original':essay['id'],'incomplete':incomplete['id'],'answerMarks':40,'printedMarks':60,'invalidChoiceStructureExportRejected':True}
report['passed']=True;(Path(os.getenv('STUDY_EDITS_REPORT',ROOT/'verification/native-plan-edits.json'))).write_text(json.dumps(report,ensure_ascii=False,indent=2));print('PASS: immutable remove/replace, local saved ZIP, invalid replacement and incomplete economics exam rejected')
