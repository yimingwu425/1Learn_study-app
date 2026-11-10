"""Real-data parity against the untouched algorithms in the independent legacy copy."""
import json,urllib.request,urllib.parse,time,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def call(port,path,body=None):
 req=urllib.request.Request(f'http://127.0.0.1:{port}{path}',data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json','X-Study-Client':'local-regression'})
 with opener.open(req,timeout=120) as r:return json.load(r)
def agent(port,tool,args):return call(port,'/api/agent/tools',{'tool':tool,'arguments':args})
results=[];failures=[]
for subject in ([] if '--kp-only' in sys.argv else ['9702','9708']):
 cases=[{}, {'paperType':'paper1'},{'paperType':'paper2'},{'yearFrom':2023,'yearTo':2025}, {'season':'w'}, {'component':'22'},{'variant':'3'}, {'sourceType':'past_paper'},{'sourceType':'specimen'},{'confidence':'medium'},{'confidence':'low'},{'needsReview':'true'},{'excludeReview':'true','excludeLowConfidence':'true','strictPaperSyllabus':'true'},{'marksMin':4,'marksMax':8},{'syllabusRef':'2.1'},{'tagId':'KP009'},{'tagId':'KP009','primaryOnly':'true'},{'syllabusRef':'2.1','primaryOnly':'true'},{'keyword':'resistance' if subject=='9702' else 'elasticity'},{'knowledgePointText':'wave' if subject=='9702' else 'demand'}]
 if subject=='9708':cases += [{'paperType':'paper2dr'},{'paperType':'paper2essay'},{'paperType':'paper2essay','yearFrom':2023}]
 else:cases += [{'paperType':'paper3'}]
 options=call(4320,'/api/filter-options?subject='+subject)
 for key,items in [('topic',options['topics']),('subtopic',options['subtopics'])]:cases.append({key:items[0]['value']})
 for case in cases:
  f={'subject':subject,**case,'limit':'500'};query=urllib.parse.urlencode(f)
  old=call(4318,'/api/study-bank-check?'+query);new=call(4320,'/api/questions?'+query)
  newkeys=[{'paperId':q['source'],'questionRef':q['question_no']} for q in new['questions']]
  ok=old['summary']['count']==new['total'] and abs(old['summary']['marks']-new['marks'])<.001 and old['questions']==newkeys
  result={'filters':f,'legacyCount':old['summary']['count'],'rustCount':new['total'],'marksEqual':abs(old['summary']['marks']-new['marks'])<.001,'orderedFirst500Equal':old['questions']==newkeys,'passed':ok}
  results.append(result)
  if not ok:failures.append(result)
  print(subject,case,'PASS' if ok else 'FAIL',new['total'],flush=True)
semantic=[]
for subject,queries in ([] if '--kp-only' in sys.argv else [('9702',['internal resistance and terminal pd','系统误差和不确定度','wave superposition']),('9708',['需求价格弹性计算','财政政策调控通货膨胀','comparative advantage'])]):
 for query in queries:
  args={'subject':subject,'query':query,'limit':10};old=agent(4318,'semantic_vector_search',args);new=agent(4320,'semantic_vector_search',args)
  fields=lambda r:[(q['paperId'],q['questionRef'],q['matchScorePct']) for q in r['results']]
  ok=fields(old)==fields(new);result={'subject':subject,'query':query,'sameRankedQuestionsAndScores':ok,'rustModel':new.get('model'),'top':fields(new)};semantic.append(result)
  if not ok:failures.append(result)
  print('semantic',subject,query,'PASS' if ok else 'FAIL',flush=True)
kp=[]
for subject in ['9702','9708']:
 for tool in ['get_kp_questions','get_kp_trend']:
  for selector in [{'tagId':'KP009'},{'syllabusRef':'2.1'},{'syllabusRef':'2.1','paperType':'paper1','yearFrom':2023,'yearTo':2026}]:
   args={'subject':subject,**selector};old=agent(4318,tool,args);new=agent(4320,tool,args)
   if tool=='get_kp_trend':ok=old['years']==new['years']
   else:
    normalize=lambda r:[(q['paperId'],q['questionRef'],q['marksTotal'],q['tagRole'],q['weightedMarks']) for q in r['questions']]
    ok=old['total']==new['total'] and normalize(old)==normalize(new)
   result={'tool':tool,'arguments':args,'passed':ok};kp.append(result)
   if not ok:failures.append(result)
   print(tool,subject,selector,'PASS' if ok else 'FAIL',flush=True)
report={'filterCases':results,'semanticCases':semantic,'kpCases':kp,'failures':failures,'passed':not failures}
ROOT.joinpath('verification/'+('native-kp-parity.json' if '--kp-only' in sys.argv else 'native-bank-parity.json')).write_text(json.dumps(report,ensure_ascii=False,indent=2))
assert not failures,f'{len(failures)} parity failures; see verification/native-bank-parity.json'
