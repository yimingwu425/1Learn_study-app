"""Real local-source preview checks; independent PyMuPDF is a development oracle."""
import pathlib,sqlite3,json,urllib.request,time,hashlib
import pymupdf
ROOT=pathlib.Path(__file__).resolve().parents[1];dest=ROOT/'verification/question-previews';dest.mkdir(exist_ok=True);base='http://127.0.0.1:4320';op=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def get(path,raw=False):
 with op.open(base+path,timeout=40) as r:return r.read() if raw else json.load(r)
db=sqlite3.connect('file:'+str(ROOT/'data/study.sqlite')+'?mode=ro',uri=True)
def qid(paper,no):
 return db.execute("select id from questions where json_extract(data,'$.source')=? and json_extract(data,'$.question_no')=?",(paper,no)).fetchone()[0]
first=get('/api/questions?subject=9702&limit=1')['questions'][0]['id']
ids=[('circuit-31','9702:10000'),('wire-32','9702:10001'),('latest-first',first),('damaged-text',qid('9702_m26_12','6')),('wrong-old-index',qid('9702_m26_12','29')),('structured',qid('9702_s20_23','1(b)(ii)')),('economics-material',qid('9708_m26_22','1(a)(i)'))]
report={'cases':[]}
for name,id in ids:
 start=time.monotonic();q=get('/api/questions/'+id);p=ROOT/'data/files'/(q['qp_file_id']+'.pdf');before=hashlib.sha256(p.read_bytes()).hexdigest();focus=get('/api/questions/'+id+'/focus');entry={'name':name,'id':id,'source':q['source'],'questionNo':q['question_no'],'originalIndexedPages':q['qp_pages'],'focus':focus}
 assert focus['located'],entry
 page=focus['page'];w,h=focus['pageWidth'],focus['pageHeight'];top=max(0,focus['y']-12);bottom=focus.get('endY',h);assert w>0 and h>0 and top+20<bottom<=h
 raw=get('/api/files/'+q['qp_file_id']+'/pages/'+str(page),True);(dest/(name+'-full.png')).write_bytes(raw)
 assert focus['renderDpi']==192 and raw[:8]==b'\x89PNG\r\n\x1a\n'
 assert abs(int.from_bytes(raw[16:20],'big')-w*192/72)<=2 and abs(int.from_bytes(raw[20:24],'big')-h*192/72)<=2
 doc=pymupdf.open(p);assert page<=len(doc);doc[page-1].get_pixmap(matrix=pymupdf.Matrix(96/72,96/72),clip=pymupdf.Rect(0,top,w,bottom)).save(dest/(name+'-source-region.png'));text=doc[page-1].get_text(clip=pymupdf.Rect(0,top,w,bottom));entry['sourceRegionTextChars']=len(text)
 if name=='circuit-31':assert 'oil droplet' not in text.lower() and '7.2 W' in text and 'variable resistor.' in text,text
 if name=='wire-32':assert 'potential difference' in text and 'oil droplet' not in text.lower() and '7.2 W' not in text,text
 if name=='wrong-old-index':assert q['qp_pages']==[15] and page==14,entry
 if name=='economics-material':assert page==2 and 'fast food' in text.lower(),entry
 assert hashlib.sha256(p.read_bytes()).hexdigest()==before;entry.update(sourceUnchanged=True,seconds=round(time.monotonic()-start,2),passed=True);report['cases'].append(entry);print(name,'PASS',entry['seconds'],focus['method'],flush=True)
report['passed']=True;report['providerConfigured']=get('/api/recognition/status')['provider_configured'];assert report['providerConfigured'] is False;(dest/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
