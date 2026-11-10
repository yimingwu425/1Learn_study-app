"""Local engine evaluation using unchanged real papers; never updates app data."""
import hashlib,json,re,subprocess
from pathlib import Path
import pymupdf
ROOT=Path(__file__).resolve().parents[1]
CLI=ROOT/'.toolchain/pdfcraft-cli/bin/pdfcraft-cli-0.20.0-macos-universal/pdfcraft-cli'
OUT=ROOT/'verification/pdfcraft-evaluation';OUT.mkdir(exist_ok=True)
CASES=[
 ('rotated-mark-scheme',Path('/Users/yimingwu/Downloads/AS Phys 9702/2020/MS/9702_s20_ms_22.pdf'),[1,2]),
 ('circuit-and-graph',Path('/Users/yimingwu/Downloads/AS Phys 9702/2023/QP/9702_w23_qp_12.pdf'),[14]),
 ('cross-page-data-response',Path('/Users/yimingwu/Downloads/AS Econ 9708/2025/QP/9708_m25_qp_22.pdf'),[2,3]),
]
def run(*args):
 p=subprocess.run([str(CLI),*map(str,args)],capture_output=True,text=True,timeout=90)
 if p.returncode:raise RuntimeError(f'PdfCraft {args[0]} failed: {p.stderr[:500]} {p.stdout[:500]}')
 return p.stdout
def words(s):return set(re.findall(r'[a-z]{2,}',s.lower()))
report=[]
for name,path,pages in CASES:
 before=hashlib.sha256(path.read_bytes()).hexdigest();out=OUT/(name+'.pdf')
 info=json.loads(run('info',path));run('extract',path,'--pages',','.join(map(str,pages)),'--out',out)
 result={'case':name,'source':str(path),'pages':pages,'info':info,'page_checks':[]}
 with pymupdf.open(path) as src,pymupdf.open(out) as extracted:
  assert len(extracted)==len(pages)
  for index,n in enumerate(pages):
   original=src[n-1];copy=extracted[index]
   a=original.get_pixmap(matrix=pymupdf.Matrix(96/72,96/72),alpha=False)
   b=copy.get_pixmap(matrix=pymupdf.Matrix(96/72,96/72),alpha=False)
   exact=(a.width,a.height,a.samples)==(b.width,b.height,b.samples)
   a.save(OUT/f'{name}-{n}-mupdf.png')
   raster=OUT/f'{name}-{n}-pdfcraft.png';run('render',path,'--page',n,'--dpi',96,'--out',raster)
   native=pymupdf.Pixmap(raster)
   dimension_match=(a.width,a.height)==(native.width,native.height)
   # Relative engine comparison only; MuPDF is not a ground-truth OCR oracle.
   text=run('text',path,'--page',n);(OUT/f'{name}-{n}-pdfcraft.txt').write_text(text)
   wa,wb=words(original.get_text()),words(text)
   overlap=len(wa&wb)/max(1,len(wa))
   check={'page':n,'rotation_before':original.rotation,'rotation_after':copy.rotation,'extraction_pixel_exact_under_mupdf':exact,'mupdf_dimensions':[a.width,a.height],'pdfcraft_dimensions':[native.width,native.height],'render_dimensions_match':dimension_match,'english_word_recall_vs_mupdf':round(overlap,4)}
   result['page_checks'].append(check)
   assert exact,check
 result['source_unchanged']=hashlib.sha256(path.read_bytes()).hexdigest()==before
 assert result['source_unchanged']
 report.append(result);print(name,result['page_checks'],flush=True)
# Combine extracted real source pages and verify order and raster preservation.
combined=OUT/'combined-real-pages.pdf';run('combine',*(OUT/(name+'.pdf') for name,_,_ in CASES),'--out',combined)
with pymupdf.open(combined) as doc:
 assert len(doc)==sum(len(pages) for _,_,pages in CASES)
 index=0
 for name,_,pages in CASES:
  with pymupdf.open(OUT/(name+'.pdf')) as source:
   for page in source:
    assert page.get_pixmap().samples==doc[index].get_pixmap().samples;index+=1
report.append({'case':'combine-real-pages','page_count':index,'raster_preserved':True})
(OUT/'report.json').write_text(json.dumps(report,ensure_ascii=False,indent=2))
print('PASS extraction and combination preserve all selected source-page rasters; inspect engine-render PNGs separately.')
