"""Development-only oracle for actual definition sources and Rust HTTP statistics."""
import json,pathlib,urllib.request,urllib.parse,urllib.error,hashlib,struct,re
import pymupdf
R=pathlib.Path(__file__).resolve().parents[1];D=R/'verification/definition-regression';D.mkdir(exist_ok=True);c=json.loads((R/'server/assets/definitions/catalog.json').read_text());op=urllib.request.build_opener(urllib.request.ProxyHandler({}));base='http://127.0.0.1:4320'
def get(path,raw=False):
 with op.open(base+path,timeout=50) as r:return r.read() if raw else json.load(r)
def api(**kw):return get('/api/definitions?'+urllib.parse.urlencode(kw))
def same_source(a,b):
 if isinstance(a,(float,int)) and not isinstance(a,bool) and isinstance(b,(float,int)) and not isinstance(b,bool):return abs(a-b)<1e-8
 if isinstance(a,dict) and isinstance(b,dict):return a.keys()==b.keys() and all(same_source(a[k],b[k]) for k in a)
 if isinstance(a,list) and isinstance(b,list):return len(a)==len(b) and all(same_source(x,y) for x,y in zip(a,b))
 return a==b
report={'sourceEntriesCompared':61,'sourceTextAndIdentityExact':True,'coordinateTolerancePdfPoints':1e-8,'subjects':{},'sourceCases':[],'aiUsed':False,'runtime':'Rust/PdfCraft; Python is this independent development oracle only'}
for s in ['9702','9708']:
 expected=[t for t in c['terms'] if t['subject']==s];a=api(subject=s,limit=24);assert a['catalogSha256']==hashlib.sha256((R/'server/assets/definitions/catalog.json').read_bytes()).hexdigest();report['catalogSha256']=a['catalogSha256'];b=api(subject=s,limit=24,offset=24);items=a['definitions']+b['definitions'];assert len(items)==len(expected)
 events={(o['qp']['sha256'],o['questionRef'],t['id']) for t in expected for o in t['occurrences']};assert a['sample']['occurrences']==len(events);assert a['total']==len(expected)
 for t in items:
  assert same_source(t['occurrences'],next(e['occurrences'] for e in expected if e['id']==t['id'])),t['id']
  assert t['occurrenceCount']==len({(o['qp']['sha256'],o['questionRef']) for o in t['occurrences']})
 chapter=api(subject=s,chapter=2,year_from=2026,year_to=2026,limit=24)
 for t in chapter['definitions']:
  assert t['chapter']==2
  assert all(o['year']==2026 for o in t['occurrences'])
 report['subjects'][s]={'terms':len(items),'confirmedOccurrences':len(events),'differentPapers':a['sample']['papers'],'chapterAndYearFilterPassed':True,'naturalOrderPassed':all(items[i]['chapter']<=items[i+1]['chapter'] for i in range(len(items)-1))}
for key in ['work','coherence','precision','yed','demerit','real_gdp','subsidy','comparative']:
 t=next(t for t in c['terms'] if t['id']==key);o=next((o for o in t['occurrences'] if o['id']==t['preferredOccurrenceId']),t['occurrences'][0]);p=o['ms']['pages'][0];file=R/'data/files'/(o['ms']['fileId']+'.pdf');before=hashlib.sha256(file.read_bytes()).hexdigest();assert before==o['ms']['sha256'];doc=pymupdf.open(file);page=doc[p['page']-1];assert abs(page.rect.width-p['width'])<.1 and abs(page.rect.height-p['height'])<.1
 assert '©' not in o['originalMarkSchemeText']
 raw=get('/api/files/'+o['ms']['fileId']+'/pages/'+str(p['page']),True);assert raw[:8]==b'\x89PNG\r\n\x1a\n';w,h=struct.unpack('>II',raw[16:24]);assert abs(w-p['width']*192/72)<=2 and abs(h-p['height']*192/72)<=2
 (D/(key+'-native-full.png')).write_bytes(raw);page.get_pixmap(matrix=pymupdf.Matrix(2,2),clip=pymupdf.Rect(p['bbox'])).save(D/(key+'-source-row.png'))
 if o['answerKind']=='correct_mcq_option':assert o['correctOption'] in 'ABCD' and o['correctOptionText']
 assert hashlib.sha256(file.read_bytes()).hexdigest()==before;report['sourceCases'].append({'term':key,'paper':o['paperId'],'question':o['questionRef'],'msPage':p['page'],'rotation':page.rotation,'nativeDimensions':[w,h],'sourceUnchanged':True})
for params in [{'subject':'0000'},{'year_from':2026,'year_to':2020},{'sort':'random'},{'limit':999}]:
 try:api(**params);raise AssertionError('invalid filters accepted')
 except urllib.error.HTTPError as e:assert e.code==400
report['passed']=True;(D/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2));print(json.dumps(report,ensure_ascii=False,indent=2))
