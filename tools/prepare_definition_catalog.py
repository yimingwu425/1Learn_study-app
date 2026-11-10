"""Development-only migration from immutable user PDFs; runtime reads JSON in Rust.
Requires the independently inspected discovery in verification and the reviewed mapping.
No AI call, original/PDF/database write, or production Python dependency.
"""
from pathlib import Path
import json,hashlib,re,sqlite3,tempfile,os
import pymupdf
ROOT=Path(__file__).resolve().parents[1];ASSETS=ROOT/'server/assets/definitions'
def norm(s):return ' '.join(s.split())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def rows(doc,ref):
 result=[]
 for p in doc:
  words=[(pymupdf.Rect(w[:4])*p.rotation_matrix,w[4]) for w in p.get_text('words')]
  anchors=[(b,t) for b,t in words if (('(' in t) if '(' in ref else True) and b.x0<130 and 45<b.y0<p.rect.height-32 and re.fullmatch(r'[1-9][0-9]?(?:\([a-z]\))?(?:\((?:i|ii|iii|iv|v|vi|vii|viii|ix|x)\))?',t)]
  for box,t in anchors:
   if t!=ref:continue
   footer=min([b.y0-3 for b,t in words if t in ['©','Page'] and b.y0>p.rect.height*.7]+[p.rect.height-40]);header=[b.y0-2 for b,t in words if t=='Question' and b.x0<140 and b.y0>box.y0+8 and any(t2=='Answer' and abs(b2.y0-b.y0)<3 for b2,t2 in words)];end=min([b.y0 for b,t in anchors if b.y0>box.y0+5]+[footer]+header);bbox=pymupdf.Rect(25,box.y0-2,p.rect.width-25,end-2)
   if bbox.height<=5:continue
   text=norm(p.get_text(clip=bbox*p.derotation_matrix))
   if '©' in text or re.search(r'Page \d+ of',text):raise ValueError('Footer leaked into source row')
   result.append({'page':p.number+1,'bbox':list(bbox),'width':p.rect.width,'height':p.rect.height,'text':text})
 return result
# Each assignment was checked against an actual definition request, not a topic tag.
MAPPING={0:['work'],1:['wavelength@4(a)(i)','amplitude@4(a)(ii)'],2:['diffraction','coherence'],3:['ohm'],4:['force'],5:['work'],6:['pd'],7:['pd'],9:['acceleration'],10:['transverse'],11:['pd'],12:['plastic'],13:['coulomb'],14:['longitudinal'],15:['velocity'],16:['work'],17:['node'],18:['precision@1(b)(i)','accuracy@1(b)(ii)'],19:['volt'],21:['density'],22:['wavelength'],23:['period'],24:['ohm'],25:['acceleration'],26:['moment'],27:['emf'],28:['velocity'],29:['cg'],30:['power'],32:['coherence'],33:['velocity'],34:['power'],36:['coherence'],37:['pressure'],38:['cg'],39:['force'],40:['frequency'],41:['young'],42:['velocity'],43:['strain'],44:['displacement'],45:['young'],46:['moment'],47:['force'],48:['stress@4(a)(i)','strain@4(a)(ii)'],49:['precision'],50:['node'],51:['ohm'],52:['cg'],53:['pd'],54:['density'],55:['momentum'],56:['longitudinal'],57:['power'],58:['work'],59:['velocity'],60:['amplitude'],61:['precision@1(b)(i)','accuracy@1(b)(ii)'],62:['power'],64:['fundamental'],65:['work'],66:['cg'],67:['pd'],68:['acceleration'],69:['efficiency'],71:['diffraction'],72:['protectionism'],74:['appreciation'],75:['depreciation'],76:['price_stability'],77:['expansionary_fiscal'],78:['subsidy'],79:['yed'],80:['demerit'],81:['consumer_surplus','producer_surplus'],82:['division_labour'],83:['demerit'],84:['deflation'],85:['xed'],86:['aggregate_supply'],87:['ca_surplus'],88:['consumer_surplus','producer_surplus'],89:['equilibrium'],90:['comparative'],91:['yed'],92:['circular_flow'],93:['protectionism'],94:['positive_statement'],95:['merit'],96:['real_gdp'],97:['proportional_tax'],98:['equilibrium@2(a)'],99:['circular_flow'],100:['budget_deficit'],101:['yed'],102:['consumer_surplus'],103:['inflation'],104:['ped'],107:['cost_push'],108:['division_labour'],109:['comparative'],110:['opportunity'],111:['ped'],112:['tax_incidence'],113:['protectionism']}
EXCLUDED={8:'Uses power to derive units; not a direct definition request',20:'Symbol identification, not this definition catalogue',31:'Power derivation already represented by its definition subpart',35:'Power-unit derivation, not an additional definition',63:'Power derivation, not an additional definition',70:'Identifies a quantity used in a derivation, not a definition of that quantity',73:'Context-specific stimulus package explanation; not accepted as a generic definition without further review',105:'Interprets a negative real interest rate; not counted as a direct definition',106:'Interprets a change in a Gini coefficient; not counted as a direct definition'}
def main():
 d=json.loads((ROOT/'verification/definition-discovery-enriched.json').read_text());syllabus=json.loads((ROOT/'server/assets/syllabus/catalog.json').read_text());db=sqlite3.connect('file:'+str(ROOT/'data/study.sqlite')+'?mode=ro',uri=True);qids={}
 for id,raw in db.execute('select id,data from questions'):
  q=json.loads(raw);qids[(q.get('source'),q.get('question_no'))]=id
 terms={}
 for line in (ASSETS/'terms.tsv').read_text().splitlines():
  if not line or line.startswith('#'):continue
  id,subject,en,cn,tag,answer,translation=line.split('|');point=syllabus[subject]['points'][tag];ref=point['syllabusRef'];chapter=int(ref.split('.')[0]);chapter_en=next(json.loads(raw)['topic'] for raw, in db.execute('select data from syllabus_points where subject=? and tag_id=?',(subject,tag)))
  terms[id]={'id':id,'subject':subject,'termEn':en,'termCn':cn,'tagId':tag,'syllabusRef':ref,'chapter':chapter,'chapterEn':chapter_en,'chapterCn':f"{chapter} {syllabus[subject]['chapters'][str(chapter)]}",'reciteEn':answer,'reciteCn':translation,'answerLabel':'据评分要求整理的背诵版，非逐字官方原文','occurrences':[]}
 files={};docs={};pending=[]
 def file(id):
  p=ROOT/'data/files'/(id+'.pdf')
  if id not in files:files[id]={'sha256':sha(p),'bytes':p.stat().st_size}
  if id not in docs:docs[id]=pymupdf.open(p)
  return docs[id],files[id]
 review_path=ASSETS/'reviewed-mapping.json'
 reviewed=json.loads(review_path.read_text()) if review_path.exists() else {}
 def candidate_key(x):return hashlib.sha256(f"{x['qp_file_id']}:{x['page']}:{norm(x['context'])}".encode()).hexdigest()
 keyed={candidate_key(x):i for i,x in enumerate(d['candidates'])}
 stable=reviewed.get('sourceAssignments')
 assignments_to_build={keyed[item['candidateId']]:item['termsAndReferences'] for item in stable} if stable else MAPPING
 for index,assignments in assignments_to_build.items():
  source=d['candidates'][index];qp,qhash=file(source['qp_file_id']);ms,mhash=file(source['ms_file_id']);p=qp[source['page']-1]
  for assignment in assignments:
   termid,_,override=assignment.partition('@');term=terms[termid];ref=override or source['question_ref'];found=rows(ms,ref)
   if not found:pending.append({'candidateIndex':index,'termId':termid,'source':source['source'],'questionRef':ref,'reason':'Corresponding MS row not located uniquely'});continue
   if not ref or not re.fullmatch(r'\d+(?:\([a-z]\))?(?:\([ivx]+\))?',ref):raise ValueError(ref)
   box=source['trigger_bbox'];qpbox=[25,max(40,box[1]-12),p.rect.width-25,min(p.rect.height-50,box[3]+180)];choice=None;option=None
   if source['source'].rsplit('_',1)[-1].startswith('1'):
    choices=set(re.findall(r'(?<!\w)([ABCD])(?!\w)',found[0]['text']));
    if len(choices)!=1:pending.append({'candidateIndex':index,'termId':termid,'source':source['source'],'questionRef':ref,'reason':'MCQ answer key not unique'});continue
    choice=choices.pop();words=p.get_text('words');labels=[w for w in words if w[4] in 'ABCD' and len(w[4])==1 and 50<w[0]<130 and w[1]>box[1]-2];labels=sorted(labels,key=lambda w:w[1]);label=next((w for w in labels if w[4]==choice),None)
    if label is None:pending.append({'candidateIndex':index,'termId':termid,'source':source['source'],'questionRef':ref,'reason':'Correct MCQ option not located'});continue
    nextq=[w[1] for w in words if w[4].isdigit() and int(w[4])>int(ref) and w[0]<90 and w[1]>label[1]+4];end=min([w[1] for w in labels if w[1]>label[1]+4]+[p.rect.height-55]+nextq);option=norm(p.get_text(clip=pymupdf.Rect(label[2]+2,label[1]-1,p.rect.width-40,end-1)))
    if not option:pending.append({'candidateIndex':index,'termId':termid,'source':source['source'],'questionRef':ref,'reason':'Empty MCQ option'});continue
   paper=source['source'];year=2000+int(paper.split('_')[1][1:]);event=hashlib.sha256(f"{qhash['sha256']}:{ref}:{termid}".encode()).hexdigest()[:24]
   occ={'id':event,'candidateIndex':index,'paperId':paper,'questionRef':ref,'questionId':qids.get((paper,ref)),'year':year,'season':paper.split('_')[1][0],'component':paper.split('_')[2],'qp':{'fileId':source['qp_file_id'],'sha256':qhash['sha256'],'page':source['page'],'bbox':qpbox,'width':p.rect.width,'height':p.rect.height},'ms':{'fileId':source['ms_file_id'],'sha256':mhash['sha256'],'pages':found},'originalMarkSchemeText':'\n'.join(y['text'] for y in found),'answerKind':'correct_mcq_option' if choice else 'written_mark_scheme','correctOption':choice,'correctOptionText':option,'questionRequest':norm(source['context']).split('...')[0],'reviewStatus':'definition_request_and_matching_official_row_checked'}
   if not any(o['id']==event for o in term['occurrences']):term['occurrences'].append(occ)
 for t in terms.values():
  if not t['occurrences']:raise ValueError('No real source for '+t['id'])
  t['occurrences'].sort(key=lambda o:(o['year'],o['paperId'],o['questionRef']))
  preferred={'yed':'9708_m26_22','ped':'9708_w25_21','demerit':'9708_s21_22','consumer_surplus':'9708_m26_22','producer_surplus':'9708_m26_22','protectionism':'9708_s24_22'}.get(t['id'])
  selected=next((o for o in t['occurrences'] if o['paperId']==preferred),None) or next((o for o in t['occurrences'] if o['answerKind']=='written_mark_scheme'),t['occurrences'][0]);t['preferredOccurrenceId']=selected['id'];t['scopeNote']='这里整理的是定义得分点；论述题仍须按题意完成分析、图示和评价。' if t['subject']=='9708' else '高/低、波型及单位等题境要求仍应对照该次原题。'
  if t['id']=='aggregate_supply':t['scopeNote']='该次原题还要求区分短期与长期总供给；定义得分点不能代替后续分析。'
  if t['id']=='real_gdp':t['sourceCaveat']='该评分条目另有疑似错误的简写算式；背诵版只采用其“经过物价变动调整”的文字定义，不采用该算式。'
 corpus=[{'paperId':x['source'],'subject':x['subject'],'pages':x.get('pages')} for x in d['scan']];out={'schemaVersion':1,'source':'Immutable locally supplied Cambridge QP/MS PDFs','scope':'271份原卷的可读文字检索及已核对匹配。并非全库穷尽识别；乱码、扫描及其他问法仍可能漏检。考频只统计下面已确认记录，不把知识点标签或推导题重复当作定义题。','dedupRule':'同一原卷文件哈希、同一小问题号、同一术语只计一次；同题要求不同定义时分别记录。不同官方卷号/变式保留为不同试卷。','candidateCount':len(d['candidates']),'excludedCandidates':[{'candidateIndex':i,'reason':v} for i,v in EXCLUDED.items()],'pendingCandidates':pending,'scannedPapers':corpus,'terms':list(terms.values())}
 stable_excluded=reviewed.get('sourceExcluded') or [{'candidateId':candidate_key(d['candidates'][i]),'reason':reason} for i,reason in EXCLUDED.items()]
 out['excludedCandidates']=stable_excluded
 known={candidate_key(d['candidates'][i]) for i in assignments_to_build}|{item['candidateId'] for item in stable_excluded}
 for source in d['candidates']:
  if candidate_key(source) not in known:out['pendingCandidates'].append({'candidateId':candidate_key(source),'source':source['source'],'reason':'New candidate has not been reviewed; excluded from frequency'})
 with tempfile.NamedTemporaryFile('w',encoding='utf-8',dir=ASSETS,prefix='.catalog-',delete=False) as temporary:
  temporary.write(json.dumps(out,ensure_ascii=False,indent=2));temporary.flush();os.fsync(temporary.fileno());name=temporary.name
 os.replace(name,ASSETS/'catalog.json');(ASSETS/'reviewed-mapping.json').write_text(json.dumps({'schemaVersion':2,'sourceAssignments':[{'candidateId':candidate_key(d['candidates'][i]),'source':d['candidates'][i]['source'],'page':d['candidates'][i]['page'],'termsAndReferences':a} for i,a in assignments_to_build.items()],'sourceExcluded':reviewed.get('sourceExcluded') or [{'candidateId':candidate_key(d['candidates'][i]),'reason':reason} for i,reason in EXCLUDED.items()]},indent=2));print(json.dumps({'terms':len(terms),'occurrences':sum(len(t['occurrences']) for t in terms.values()),'bySubject':{s:sum(t['subject']==s for t in terms.values()) for s in ['9702','9708']},'pending':pending},ensure_ascii=False,indent=2))
if __name__=='__main__':main()
