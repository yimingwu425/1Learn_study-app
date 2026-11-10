"""Exercise real QP/MS groups, default marks, bilingual selections and source export."""
import json,urllib.request,zipfile,io,sys,time,os
from pathlib import Path
import pymupdf
ROOT=Path(__file__).resolve().parents[1];DATA=Path(os.environ.get('STUDY_CHECK_DATA',ROOT/'data'));dest=Path(os.environ.get('STUDY_CHECK_DEST',ROOT/'verification/native-composition'));dest.mkdir(parents=True,exist_ok=True)
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def call(path,body=None):
 req=urllib.request.Request(os.environ.get('STUDY_CHECK_BASE','http://127.0.0.1:4320')+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','X-Study-Client':'native-composer-check'})
 try:
  with opener.open(req,timeout=180) as r:return r.read()
 except urllib.error.HTTPError as e:raise RuntimeError(str(e)+' '+e.read().decode()[:1000]) from e
cases=[('physics-mcq',{'subject':'9702','paperType':'paper1','mode':'assignment','targetMarks':10},10,10,True),('physics-exam',{'subject':'9702','paperType':'paper1','mode':'paper'},40,40,False),('physics-structured',{'subject':'9702','paperType':'paper2','mode':'paper'},60,60,True),('physics-practical',{'subject':'9702','paperType':'paper3','mode':'paper'},40,40,True),('economics-mcq',{'subject':'9708','paperType':'paper1','mode':'paper'},30,30,False),('economics-data',{'subject':'9708','paperType':'paper2dr','mode':'paper'},20,20,True),('economics-essays',{'subject':'9708','paperType':'paper2essay','mode':'paper'},40,80,True),('economics-full',{'subject':'9708','paperType':'paper2','mode':'paper'},60,100,True),('physics-weighted',{'subject':'9702','paperType':'paper1','mode':'assignment','targetMarks':12,'sections':'10.1,10.2','ratioText':'10.1:50,10.2:50'},12,12,False),('physics-kp',{'subject':'9702','paperType':'paper1','mode':'assignment','targetMarks':5,'basis':'knowledgePoint','ratioText':'KP120:100'},5,5,False)]
previous={r['name']:r for r in json.loads((dest/'report.json').read_text())} if '--exports-only' in sys.argv else {}
results=[]
for name,args,answer,printed,export in cases:
 started=time.monotonic();entry={'name':name,'input':args};print('START',name,flush=True)
 try:
  r=previous[name]['summary'] if previous else json.loads(call('/api/compositions',args));entry['summary']=r
  assert r['selectionEngine']=='rust' and r['exactTarget'] and r['actualMarks']==answer and r['printedMarks']==printed,r
  p=json.loads((DATA/'compositions'/f"{r['id']}.json").read_text());members=[(m['paperId'],m['questionRef']) for q in p['questions'] for m in q['members']];assert len(members)==len(set(members))
  entry['memberCount']=len(members);entry['membersUnique']=True
  if args.get('sections'):
   allowed=set(args['sections'].split(','));assert all(any(ref.rsplit('.',1)[0] in allowed for ref in m['syllabusRefs'].split(';')) for q in p['questions'] for m in q['members'])
  if export or (os.getenv('STUDY_CHECK_FULL_MCQ')=='1' and name in ('physics-exam','economics-mcq')):
   data=call(r['downloadUrl']);(dest/f'{name}.zip').write_bytes(data)
   z=zipfile.ZipFile(io.BytesIO(data));entry['files']=z.namelist();pdfs={}
   for filename in z.namelist():
    if filename.endswith('.pdf'):
     raw=z.read(filename);doc=pymupdf.open(stream=raw,filetype='pdf');assert len(doc)>0;pdfs[filename]={'pages':len(doc),'textChars':sum(len(page.get_text()) for page in doc)}
     page=min(1,len(doc)-1);doc[page].get_pixmap(matrix=pymupdf.Matrix(1,1),alpha=False).save(dest/(f'{name}-'+('answers' if 'teacher' in filename else 'student')+'.png'))
   entry['pdfs']=pdfs;assert len(pdfs)==2
   manifest=json.loads(z.read(next(f for f in z.namelist() if f.endswith('manifest.json'))));assert manifest['actualMarks']==answer and manifest['printedMarks']==printed
  entry['passed']=True
 except Exception as e:entry['passed']=False;entry['error']=str(e)
 entry['seconds']=round(time.monotonic()-started,2);results.append(entry);(dest/'report.json').write_text(json.dumps(results,ensure_ascii=False,indent=2));print(name,'PASS' if entry['passed'] else 'FAIL',entry['seconds'],entry.get('error','')[:600],flush=True)
assert all(r['passed'] for r in results),'See verification/native-composition/report.json'
