"""Development-only source discovery; never modifies originals or publishes candidates."""
import pathlib,json,pymupdf,re
r=pathlib.Path(__file__).resolve().parents[1]
# Repeatable read-only discovery. Review decisions use source fingerprints, not list positions.
import sqlite3
con=sqlite3.connect('file:'+str(r/'data/study.sqlite')+'?mode=ro',uri=True);papers={}
for raw, in con.execute('select data from questions'):
 q=json.loads(raw)
 if q.get('qp_file_id') and q.get('ms_file_id'):papers[q['source']]=(q['subject'],q['qp_file_id'],q['ms_file_id'])
found=[];coverage=[];pattern=re.compile(r'\b(?:define\b|definition\s+of\b|(?:state|explain|what)\s+(?:is\s+)?(?:what\s+is\s+)?meant\s+by\b)',re.I)
for paper,(subject,qp,ms) in sorted(papers.items()):
 with pymupdf.open(r/'data/files'/(qp+'.pdf')) as pdf:
  pages=[]
  for p in pdf:
   text=p.get_text();matches=list(pattern.finditer(text))
   if matches:pages.append(p.number+1)
   for m in matches:found.append({'source':paper,'subject':subject,'qp_file_id':qp,'ms_file_id':ms,'page':p.number+1,'context':text[max(0,m.start()-100):m.end()+260]})
  coverage.append({'source':paper,'subject':subject,'pages':len(pdf),'definitionPages':pages})
d={'candidates':found,'scan':coverage,'scope':'Readable PDF text search, not verified frequency counts. Topic tags alone are never counted.'};(r/'verification').mkdir(exist_ok=True);(r/'verification/definition-discovery.json').write_text(json.dumps(d,ensure_ascii=False,indent=2))
cache={};by_page={};states={}
def doc(id):
 if id not in cache:cache[id]=pymupdf.open(r/'data/files'/(id+'.pdf'))
 return cache[id]
for source,x in {x['source']:x for x in d['candidates']}.items():
 state=['','',''];qleft=72;page_records=[]
 for p in doc(x['qp_file_id']):
  events=[]
  for block in p.get_text('dict')['blocks']:
   for line in block.get('lines',[]):
    for s in line.get('spans',[]):
     if re.fullmatch('[1-9][0-9]?',s['text'].strip()) and 'bold' in s['font'].lower() and 20<s['bbox'][0]<100 and 48<s['bbox'][1]<p.rect.height-45:events.append((s['bbox'][1],s['bbox'][0],'main',s['text'].strip()))
  if events:qleft=min(e[1] for e in events)
  for w in p.get_text('words'):
   a,b,_,_,t,*_=w
   if b<48 or b>p.rect.height-45:continue
   if qleft+9<a<qleft+40 and re.fullmatch(r'\([a-h]\)',t):events.append((b,a,'part',t))
   elif qleft+30<a<qleft+85 and re.fullmatch(r'\((?:i|ii|iii|iv|v|vi|vii|viii|ix|x)\)',t):events.append((b,a,'roman',t))
  labels=[]
  for y,a,kind,t in sorted(events,key=lambda e:(round(e[0]/3),e[1])):
   if kind=='main':state=[t,'','']
   elif kind=='part':state=[state[0],t,'']
   else:state=[state[0],state[1],t]
   labels.append((y,a,state.copy()))
  by_page[(source,p.number+1)]=labels;page_records.append(state.copy())
 states[source]=page_records
for i,x in enumerate(d['candidates']):
 p=doc(x['qp_file_id'])[x['page']-1];text=p.get_text();pat=re.compile(r'\b(?:define\b|definition\s+of\b|(?:state|explain|what)\s+(?:is\s+)?(?:what\s+is\s+)?meant\s+by\b)',re.I);matches=list(pat.finditer(text));m=next((m for m in matches if text[max(0,m.start()-100):m.end()+260]==x['context']),None)
 if not m:continue
 rects=p.search_for(m.group(0));prior=sum(1 for prev in matches if prev.start()<m.start() and prev.group(0).lower()==m.group(0).lower());rect=rects[min(prior,len(rects)-1)] if rects else None
 if rect:
  state=states[x['source']][x['page']-2].copy() if x['page']>1 else ['','','']
  for y,a,new in by_page[(x['source'],x['page'])]:
   if y<rect.y0+2:state=new
  ref=''.join(state);x['question_ref']=ref;x['trigger_bbox']=list(rect);rows=[]
  for mp in doc(x['ms_file_id']):
   words=[(list(pymupdf.Rect(w[:4])*mp.rotation_matrix),w[4]) for w in mp.get_text('words')]
   anchors=[(box,t) for box,t in words if (('(' in t) if '(' in ref else True) and box[0]<130 and 45<box[1]<mp.rect.height-32 and re.fullmatch(r'[1-9][0-9]?(?:\([a-z]\))?(?:\((?:i|ii|iii|iv|v|vi|vii|viii|ix|x)\))?',t)]
   for box,t in anchors:
    if t!=ref:continue
    footer=min([b[1]-3 for b,t in words if t in ['©','Page'] and b[1]>mp.rect.height*.7]+[mp.rect.height-40]);nexty=min([b[1] for b,t in anchors if b[1]>box[1]+5]+[footer]);clip=pymupdf.Rect(25,box[1]-2,mp.rect.width-25,nexty-2);rows.append({'page':mp.number+1,'bbox':list(clip),'width':mp.rect.width,'height':mp.rect.height,'rotation':mp.rotation,'text':mp.get_text(clip=clip*mp.derotation_matrix)})
  x['ms_rows']=rows
 print(i,x['source'],x.get('question_ref','?'),'MS',[(y['page'],y['text'].replace('\n',' ')[:180])for y in x.get('ms_rows',[])])
(r/'verification/definition-discovery-enriched.json').write_text(json.dumps(d,ensure_ascii=False,indent=2))
