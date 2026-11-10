#!/usr/bin/env python3
"""Render immutable evidence for a future independent visual verifier."""
import hashlib,json,math,sys
from pathlib import Path
import pymupdf as fitz
from inspect_pdf import open_pdf,IntakeError
from render_pdf_page import render_region,RENDER_VERSION

def main():
 if len(sys.argv)!=5:raise IntakeError('Usage: render_agent_region.py source.pdf page bbox-json destination-directory')
 path,page,bbox,dest=Path(sys.argv[1]),int(sys.argv[2]),json.loads(sys.argv[3]),Path(sys.argv[4])
 if not isinstance(bbox,list) or len(bbox)!=4 or any(not isinstance(x,(float,int)) or isinstance(x,bool) or not math.isfinite(x) for x in bbox):raise IntakeError('Invalid rectangle')
 with open_pdf(path) as doc:
  if not 1<=page<=len(doc):raise IntakeError('Page out of range')
  p=doc[page-1];bounds=fitz.Rect(0,0,p.cropbox.width,p.cropbox.height);box=fitz.Rect(bbox)
  if box.is_empty or not bounds.contains(box):raise IntakeError('Rectangle out of range')
  png=render_region(p,bbox)
  digest=hashlib.sha256(png).hexdigest();dest.mkdir(parents=True,exist_ok=True);out=dest/(digest+'.png');out.write_bytes(png)
  print(json.dumps({'image_sha256':digest,'render_version':RENDER_VERSION,'page':page,'bbox':bbox,'filename':out.name}))
if __name__=='__main__':
 try:main()
 except Exception as e:print(json.dumps({'error':str(e)}));sys.exit(2)
