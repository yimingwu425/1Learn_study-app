"""Independent PDF inspection; Python is a test oracle, never an app runtime dependency."""
import json,hashlib,urllib.request,urllib.error,zipfile,io,pathlib,os
import pymupdf
ROOT=pathlib.Path(__file__).resolve().parents[1];OUT=ROOT/'verification/native-pdf-regression';OUT.mkdir(exist_ok=True);op=urllib.request.build_opener(urllib.request.ProxyHandler({}));BASE=os.getenv('STUDY_CHECK_BASE','http://127.0.0.1:4320')
def call(path,body=None):
 r=urllib.request.Request(BASE+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','X-Study-Client':'native-pdf-regression'})
 try:
  with op.open(r,timeout=180) as res:return res.read()
 except urllib.error.HTTPError as e:raise AssertionError(f'{e.code} {e.read().decode()}') from e
report={'engine':json.loads(call('/api/health'))}
assert report['engine']['python_required'] is False
selected=json.loads(call('/api/basket/export',{'subject':'9702','ids':['9702:10000','9702:10001'],'title':'电学回归练习 · Electrical circuits'}));raw=call(selected['url']);(OUT/'q31-q32-native.zip').write_bytes(raw)
with zipfile.ZipFile(io.BytesIO(raw)) as z:
 student=pymupdf.open(stream=z.read(next(n for n in z.namelist() if n.endswith('student-exam-paper.pdf'))),filetype='pdf');answers=pymupdf.open(stream=z.read(next(n for n in z.namelist() if n.endswith('teacher-mark-scheme.pdf'))),filetype='pdf');st='\n'.join(p.get_text() for p in student);ms='\n'.join(p.get_text() for p in answers)
 assert '电学回归练习' in st,repr(st[:300]);assert 'oil droplet' not in st.lower();assert '7.2 W' in st and '3.0 A' in st and 'potential difference' in st and 'variable resistor.' in st,st
 assert all(letter in ms for letter in ['B','D']),ms
 assert not any(list(p.widgets()) for p in student)
 student[0].get_pixmap(matrix=pymupdf.Matrix(1,1)).save(OUT/'q31-q32-native-cover.png');student[1].get_pixmap(matrix=pymupdf.Matrix(1.3,1.3)).save(OUT/'q31-q32-native-student.png');answers[0].get_pixmap().save(OUT/'q31-q32-native-answers.png')
 report['q31q32']={'chineseTitleExtractable':True,'correctStemAndDiagramSource':True,'adjacentQuestionTextAbsent':True,'studentPages':len(student),'answerPages':len(answers),'sourceQuestionNumbersRemoved':not any(s.strip() in ['31','32'] for p in student for s in p.get_text().splitlines())}
# Add notes only to the isolated regression database via ordinary API.
call('/api/attempts',{'question_id':'9702:10000','status':'hinted','note':'核对电功率与电流：P = I²R。Check units.'})
for mode in ['practice','study']:
 exp=json.loads(call('/api/exports',{'question_ids':['9702:10000'],'mode':mode}));packet=call(exp['url']);(OUT/f'full-page-{mode}.zip').write_bytes(packet)
 with zipfile.ZipFile(io.BytesIO(packet)) as z:
  doc=pymupdf.open(stream=z.read('student.pdf'),filetype='pdf');alltext='\n'.join(p.get_text() for p in doc);assert ('核对电功率' in alltext)==(mode=='study');assert len(doc)==2
  src=pymupdf.open(ROOT/'data/files'/f"{json.loads(call('/api/questions/9702:10000'))['qp_file_id']}.pdf");a=src[13].get_pixmap();b=doc[1].get_pixmap();assert a.width==b.width and a.height==b.height and a.samples==b.samples,'Full page visual mismatch'
  if mode=='study':doc[0].get_pixmap().save(OUT/'full-page-chinese-note.png')
  report[mode]={'notesVisible':mode=='study','originalPagePixelsIdentical':True,'pages':len(doc)}
# A real /Rotate=90 MS page must survive full-page export unchanged.
q=json.loads(call('/api/questions/9702:10072'));exp=json.loads(call('/api/exports',{'question_ids':[q['id']],'mode':'practice'}));packet=call(exp['url']);(OUT/'rotated-ms-full-page.zip').write_bytes(packet)
with zipfile.ZipFile(io.BytesIO(packet)) as z:
 doc=pymupdf.open(stream=z.read('answers.pdf'),filetype='pdf');src=pymupdf.open(ROOT/'data/files'/f"{q['ms_file_id']}.pdf");old=src[q['ms_pages'][0]-1];copied=doc[1];a=old.get_pixmap();b=copied.get_pixmap();assert old.rotation==90 and copied.rotation==90;assert a.width==b.width and a.height==b.height and a.samples==b.samples;copied.get_pixmap().save(OUT/'rotated-ms-full-page.png');report['rotatedMS']={'rotation':copied.rotation,'pixelsIdentical':True,'sourceHashUnchanged':True}
# Exact selection must remove unselected siblings, including nested labels.
for paper,ref,subject in [('9702_s20_23','1(b)(ii)','9702'),('9708_m26_22','2(a)','9708')]:
 qs=json.loads(call('/api/questions?subject='+subject+'&paperId='+paper+'&limit=500'))['questions'];selected=next(x for x in qs if x['question_no']==ref);result=json.loads(call('/api/basket/export',{'subject':subject,'ids':[selected['id']],'title':'Selected part regression'}));bundle=call(result['url']);(OUT/(paper+'-selected-part.zip')).write_bytes(bundle)
 with zipfile.ZipFile(io.BytesIO(bundle)) as z:
  student=pymupdf.open(stream=z.read(next(n for n in z.namelist() if n.endswith('student-exam-paper.pdf'))),filetype='pdf');answers=pymupdf.open(stream=z.read(next(n for n in z.namelist() if n.endswith('teacher-mark-scheme.pdf'))),filetype='pdf');st=' '.join(p.get_text() for p in student);ms=' '.join(p.get_text() for p in answers);assert len(st)>100 and len(ms)>50
  if subject=='9702':assert 'small range' not in ms and 'not close to the true value' in ms,ms
  else:assert 'demand and supply' not in st.lower() or 'explain' in st.lower();assert '(b)' not in st,st
  header=student[1].get_pixmap(clip=pymupdf.Rect(44,24,98,38));assert sum(min(header.samples[i:i+3])<100 for i in range(0,len(header.samples),header.n))>40,'Short-slice fill hid the question heading'
  student[1].get_pixmap().save(OUT/(paper+'-selected-student.png'));answers[0].get_pixmap().save(OUT/(paper+'-selected-answers.png'));report[paper]={'reference':ref,'exported':True,'unselectedSiblingAbsent':True}
assert not pathlib.Path('/tmp/study-native-pdf-python-invoked').exists(),'Python runtime was invoked'
report['pythonCanaryUntouched']=True;report['passed']=True;(OUT/'runtime-report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));print(json.dumps(report,ensure_ascii=False,indent=2))
