"""Build display overlays from the user's existing syllabus PDFs. Original catalog rows stay intact."""
from pathlib import Path
import hashlib,json,re,shutil,sqlite3
import pymupdf
ROOT=Path(__file__).resolve().parents[1]
DB=sqlite3.connect(f'file:{ROOT}/data/study.sqlite?mode=ro',uri=True)
ASSETS=ROOT/'server/assets/syllabus';ASSETS.mkdir(exist_ok=True)
chapter_cn={'9702':['物理量与单位','运动学','动力学','力、密度与压强','功、能量与功率','固体形变','波动','叠加','电学','直流电路','粒子物理'],'9708':['基本经济概念与资源配置','价格体系与微观经济','政府微观经济干预','宏观经济','政府宏观经济干预','国际经济问题']}
sources={'9708':Path('/Users/yimingwu/Downloads/AS Econ 9708/Syllabus/9708_2026-2028_Syllabus.pdf'),'9702':Path('/Users/yimingwu/Downloads/AS Phys 9702/Syllabus/9702_2025-2027_Syllabus.pdf')}
out={}
for subject,source in sources.items():
 doc=pymupdf.open(source);objectives={}
 if subject=='9708':
  for i in range(14,23):
   current=None
   for line in doc[i].get_text().splitlines():
    line=line.replace('\x07','').strip()
    m=re.match(r'^([1-6]\.\d+\.\d+)\s*(.*)',line)
    if m:current=m[1];objectives[current]={'en':m[2],'page':i+1}
    elif re.match(r'^\d+(?:\.\d+)?\s*$',line) or re.match(r'^\d+\.\d+\s+',line):current=None
    elif current and line:objectives[current]['en']+=' '+line
 else:
  for i in range(15,25):
   section=None;current=None
   for block in doc[i].get_text('dict')['blocks']:
    for line in block.get('lines',[]):
     x,y,_,_=line['bbox'];s=''.join(t['text'] for t in line['spans']).replace('\x07','').strip()
     if not 75<y<790:continue
     m=re.match(r'^(\d+\.\d+)\s+',s)
     if m:section=m[1];current=None;continue
     if abs(x-62.3622)<1 and s.isdigit():
      if any(t['flags']&16 for t in line['spans']):current=None;section=None;continue
      if section:current=f'{section}.{s}';objectives[current]={'en':'','page':i+1}
     elif current and s and not s.startswith('Candidates should'):
      objectives[current]['en']+=' '+s
  # Formula glyphs are separate PDF spans; these are text transcriptions of the formulas on the cited pages.
  formulas={'5.2.1':'derive, using W = Fs, the formula ΔEₚ = mgΔh for gravitational potential energy changes in a uniform gravitational field',
   '5.2.3':'derive, using the equations of motion, the formula for kinetic energy Eₖ = ½mv²',
   '5.2.4':'recall and use Eₖ = ½mv²',
   '6.2.4':'recall and use Eₚ = ½Fx = ½kx² for a material deformed within its limit of proportionality',
   '9.2.3':'recall and use P = VI, P = I²R and P = V²/R',
   '11.1.11':'represent α- and β-decay by a radioactive decay equation of the form ²³⁸₉₂U → ²³⁴₉₀Th + ⁴₂α'}
  for ref,en in formulas.items():objectives[ref]['en']=en
 catalog=[json.loads(r[0]) for r in DB.execute('select data from syllabus_points where subject=? order by tag_id',(subject,))]
 cn=(ROOT/'server/assets/economics-zh.txt').read_text().splitlines() if subject=='9708' else [p['knowledge_point_cn'] for p in catalog]
 assert len(cn)==len(catalog),(subject,len(cn),len(catalog))
 corrections={'KP008':'区分精密度（precision）与准确度（accuracy）','KP011':'共面矢量的相加与相减','KP030':'理解受到阻力的运动物体可能达到终端（恒定）速度','KP033':'熟记弹性碰撞中总动能守恒，且相对接近速率等于相对分离速率','KP041':'用矢量三角形表示处于平衡的共面力','KP079':'熟记并运用行波强度 = 功率／面积，以及强度 ∝ 振幅²'}
 points={}
 for p,zh in zip(catalog,cn):
  ref=p['syllabus_ref'];assert ref in objectives,(subject,ref)
  en=' '.join(objectives[ref]['en'].split());assert en and len(en)>8,(subject,ref,en)
  if subject=='9702':zh=corrections.get(p['tag_id'],zh)
  points[p['tag_id']]={'en':en,'cn':zh,'page':objectives[ref]['page'],'syllabusRef':ref}
 out[subject]={'source':source.name,'sourceSha256':hashlib.sha256(source.read_bytes()).hexdigest(),'translationNote':'中文为对照英文考纲核对的学习释义，并非 Cambridge 官方译文。公式的独立字形转写为可读文字。','points':points,'chapters':{str(i+1):title for i,title in enumerate(chapter_cn[subject])}}
 shutil.copyfile(source,ASSETS/f'{subject}.pdf')
 print(subject,len(points),'bilingual objectives')
(ASSETS/'catalog.json').write_text(json.dumps(out,ensure_ascii=False,indent=2))
