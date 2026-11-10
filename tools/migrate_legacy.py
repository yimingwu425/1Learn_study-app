#!/usr/bin/env python3
"""Read-only legacy migration. Dry-run by default; never overwrites existing questions."""
import argparse, hashlib, json, os, re, sqlite3, tempfile, uuid
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote

def digest(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for b in iter(lambda:f.read(1024*1024), b''): h.update(b)
    return h.hexdigest()

def locate(root, name, subject):
    if not name: return None
    rel=Path(name)
    if rel.is_absolute() or '..' in rel.parts: return None
    candidates=[root/rel]
    parts=rel.parts
    if len(parts)>=2 and parts[:2]==('data',subject): candidates.append(root/Path(*parts[2:]))
    for p in candidates:
        p=p.resolve()
        if p.is_relative_to(root) and p.is_file(): return p
    return None

def tag_fields(tags):
    def unique(values):
        return list(dict.fromkeys(str(v).strip() for v in values if v is not None and str(v).strip()))
    labels=[]
    for tag in tags:
        label=next((str(tag[k]).strip() for k in ('knowledge_point_cn','knowledge_point_en','subtopic_cn','subtopic','topic_cn','topic') if tag.get(k) and str(tag[k]).strip()), None)
        if label: labels.append(label)
    return {'topics':unique(labels),'syllabus_refs':unique(t.get('syllabus_ref') for t in tags),'tag_ids':unique(t.get('tag_id') for t in tags)}

def paper_fields(paper):
    # Legacy component is typically 1/2/3; some source schemas carry 12/22/32.
    component=str(paper.get('component') or '').strip()
    if not component:
        component=str(paper.get('paper_id','')).rsplit('_',1)[-1]
    match=re.fullmatch(r'(?:paper)?([1-6])(?:[0-9])?',component)
    year=paper.get('year')
    try: year=int(year) if year is not None else None
    except (ValueError,TypeError): year=None
    return {'year':year,'paper_type':'paper'+match.group(1) if match else None}

def corrupt_text_encoding(text):
    # Control characters excluding ordinary whitespace are strong encoding evidence.
    bad=sum(ord(c)<32 and c not in '\n\r\t' or c=='\ufffd' for c in text)
    return bad>=5 and bad/max(len(text),1)>0.01

def migrate(args):
    src=Path(args.db).resolve(); dest=Path(args.dest).resolve(); root=Path(args.pdf_root).resolve()
    target=dest/'study.sqlite'
    if target.resolve()==src: raise ValueError('Source and destination database must differ')
    before=digest(src)
    old=sqlite3.connect('file:'+quote(str(src))+'?mode=ro',uri=True); old.row_factory=sqlite3.Row
    report={'mode':'apply' if args.apply else 'dry_run','subject':args.subject,'source_db_sha256':before,'source_database':str(src),'destination':str(dest),'questions_seen':0,'inserted':0,'would_insert':0,'skipped_existing':0,'missing_qp':0,'missing_ms':0,'files':[], 'warnings':[]}
    existing=set()
    if target.exists():
        with sqlite3.connect('file:'+quote(str(target))+'?mode=ro',uri=True) as c:
            if c.execute("SELECT 1 FROM sqlite_master WHERE name='questions'").fetchone(): existing={r[0] for r in c.execute('SELECT id FROM questions')}
    assets={}; known={}
    if (dest/'files').exists():
        for p in (dest/'files').glob('*.pdf'):
            try: uuid.UUID(p.stem)
            except ValueError: continue
            known.setdefault(digest(p),p.stem)
    def asset(name):
        p=locate(root,name,args.subject)
        if p is None:return None
        h=digest(p)
        if h not in assets:
            with p.open('rb') as f:
                if not f.read(8).startswith(b'%PDF-'): report['warnings'].append('Invalid PDF: '+str(p)); return None
            fid=known.get(h,str(uuid.uuid5(uuid.NAMESPACE_URL,'study-pdf-sha256:'+h)))
            assets[h]={'id':fid,'sha256':h,'size':p.stat().st_size,'source_path':str(p),'path':p,'reused':h in known}
        return assets[h]
    def text_page(a,page):
        if not a or not page:return '', 'source_pdf_unavailable' if not a else 'invalid_page'
        try:
            import pymupdf as fitz
            with fitz.open(a['path']) as doc:
                if not 1<=page<=len(doc):return '', 'invalid_page'
                return doc[page-1].get_text(), 'whole_page_includes_adjacent_content'
        except ImportError:return '', 'extractor_unavailable'
        except Exception as e:return '', 'extraction_failed:'+type(e).__name__
    questions=[]
    for row in old.execute('SELECT * FROM questions ORDER BY question_id'):
        q=dict(row); report['questions_seen']+=1; qid=f"{args.subject}:{q['question_id']}"
        if qid in existing: report['skipped_existing']+=1;continue
        paperrow=old.execute('SELECT * FROM papers WHERE paper_id=?',(q['paper_id'],)).fetchone()
        paper=dict(paperrow) if paperrow else {}
        tags=[dict(r) for r in old.execute('SELECT qt.*,kp.* FROM question_tags qt JOIN knowledge_points kp ON kp.tag_id=qt.tag_id WHERE qt.question_id=?',(q['question_id'],))]
        qp=asset(paper.get('qp_file'));ms=asset(paper.get('ms_file'))
        report['missing_qp']+=qp is None;report['missing_ms']+=ms is None
        def pages(key):
            v=q.get(key)
            return [int(v)] if isinstance(v,(int,float)) and v>0 and int(v)==v else []
        qpages=pages('qp_pdf_page');mpages=pages('ms_pdf_page')
        text,scope=text_page(qp,qpages[0] if qpages else None)
        raw_diagnostic=None
        if corrupt_text_encoding(text):
            raw_diagnostic=text
            text='原卷文本编码损坏，待视觉识别。请查看原始PDF。'
            scope='corrupt_text_encoding_requires_vision'
        if scope=='invalid_page': qpages=[]  # Preserve raw page in legacy_metadata, never expose a bogus mapping
        value={'id':qid,'subject':args.subject,'title':f"{q['paper_id']} · {q['question_ref']}",'source':q['paper_id'],'question_no':q['question_ref'],'marks':q.get('marks_total'),'text':text,'text_scope':scope,'summary':q.get('evidence') or '', 'status':'legacy_unreviewed','kind':'real',**tag_fields(tags),**paper_fields(paper),'source_pdf_unavailable':qp is None,'qp_file_id':qp['id'] if qp else None,'ms_file_id':ms['id'] if ms else None,'qp_pages':qpages,'ms_pages':mpages,'qp_url':f"/api/files/{qp['id']}#page={qpages[0]}" if qp and qpages else None,'ms_url':f"/api/files/{ms['id']}#page={mpages[0]}" if ms and mpages else None,'legacy_metadata':{'question':q,'paper':paper,'tags':tags,'raw_text_diagnostic':raw_diagnostic},'migration':{'source_db_sha256':before,'text_notice':'Full PDF page extraction, not a verified question crop. May include adjacent questions.'}}
        questions.append(value)
    catalog=[dict(row) for row in old.execute('SELECT * FROM knowledge_points')]
    report['catalog_points']=len(catalog)
    old.close();report['would_insert']=len(questions)
    if digest(src)!=before:raise RuntimeError('Source database changed during read; rerun from stable snapshot')
    if args.apply:
        dest.mkdir(parents=True,exist_ok=True);(dest/'files').mkdir(exist_ok=True)
        for h,a in assets.items():
            out=dest/'files'/(a['id']+'.pdf')
            if out.exists():
                if digest(out)!=h:raise RuntimeError('Destination PDF UUID collision; refusing overwrite')
                continue
            fd,tmp=tempfile.mkstemp(dir=dest/'files',prefix='.migration-')
            try:
                with os.fdopen(fd,'wb') as f, a['path'].open('rb') as r:
                    for b in iter(lambda:r.read(1024*1024),b''):f.write(b)
                if digest(Path(tmp))!=h:raise RuntimeError('Copy hash mismatch')
                try:os.link(tmp,out)
                except FileExistsError:
                    if digest(out)!=h:raise RuntimeError('Concurrent conflicting PDF')
            finally:os.unlink(tmp)
        with sqlite3.connect(target) as new:
            new.execute('CREATE TABLE IF NOT EXISTS questions(id TEXT PRIMARY KEY, subject TEXT NOT NULL, data TEXT NOT NULL)')
            new.execute('CREATE TABLE IF NOT EXISTS syllabus_points(subject TEXT NOT NULL, tag_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(subject,tag_id))')
            for point in catalog:
                new.execute('INSERT OR IGNORE INTO syllabus_points VALUES(?,?,?)',(args.subject,point['tag_id'],json.dumps(point,ensure_ascii=False)))
            for q in questions:
                cursor=new.execute('INSERT OR IGNORE INTO questions(id,subject,data) VALUES(?,?,?)',(q['id'],args.subject,json.dumps(q,ensure_ascii=False)))
                report['inserted']+=cursor.rowcount
    report['files']=[{k:v for k,v in a.items() if k!='path'} for a in assets.values()]
    report['source_db_unchanged']=digest(src)==before
    if args.apply:
        reports=dest/'migration-reports';reports.mkdir(exist_ok=True)
        rp=reports/(datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S')+'-'+str(uuid.uuid4())+'.json')
        report['report_path']=str(rp)
        with rp.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2)
    return report

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--db',required=True);p.add_argument('--subject',choices=['9702','9708'],required=True);p.add_argument('--pdf-root',required=True);p.add_argument('--dest',required=True);p.add_argument('--apply',action='store_true')
    args=p.parse_args()
    try: print(json.dumps(migrate(args),ensure_ascii=False,indent=2))
    except (ValueError,RuntimeError,sqlite3.Error,OSError) as e:p.exit(1,str(e)+'\n')
if __name__=='__main__':main()
