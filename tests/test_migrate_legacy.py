import importlib.util,json,sqlite3,tempfile,unittest,sys
from argparse import Namespace
from pathlib import Path
spec=importlib.util.spec_from_file_location('migrate',Path(__file__).parents[1]/'tools/migrate_legacy.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class MigrationTest(unittest.TestCase):
 def test_migration(self):
  with tempfile.TemporaryDirectory() as tmp:
   base=Path(tmp);src=base/'legacy.sqlite';root=base/'pdf';root.mkdir();dest=base/'new'
   with sqlite3.connect(src) as c:
    c.executescript('CREATE TABLE papers(paper_id TEXT,qp_file TEXT,ms_file TEXT,year INTEGER,component TEXT);CREATE TABLE questions(question_id INTEGER,paper_id TEXT,question_ref TEXT,qp_pdf_page INTEGER,ms_pdf_page INTEGER,marks_total REAL,evidence TEXT);CREATE TABLE knowledge_points(tag_id TEXT,topic TEXT,syllabus_ref TEXT);CREATE TABLE question_tags(question_id INTEGER,tag_id TEXT,tag_role TEXT);')
    c.execute('INSERT INTO papers VALUES(?,?,?,?,?)',('paper','data/9702/qp.pdf','data/9702/ms.pdf',2025,'22'))
    c.execute('INSERT INTO questions VALUES(1,"paper","1(a)",1,1,2,"summary not body")');c.execute('INSERT INTO knowledge_points VALUES("k1","Motion","1.1")');c.execute('INSERT INTO question_tags VALUES(1,"k1","primary")')
   args=Namespace(db=str(src),subject='9702',pdf_root=str(root),dest=str(dest),apply=False)
   before=m.digest(src);r=m.migrate(args);self.assertEqual(r['would_insert'],1);self.assertFalse(dest.exists())
   args.apply=True;r=m.migrate(args);self.assertEqual(r['inserted'],1)
   with sqlite3.connect(dest/'study.sqlite') as c:q=json.loads(c.execute('select data from questions').fetchone()[0]);self.assertEqual(q['text'],'');self.assertEqual(q['status'],'legacy_unreviewed');self.assertTrue(q['source_pdf_unavailable']);self.assertEqual(q['summary'],'summary not body');self.assertEqual(q['topics'],['Motion']);self.assertEqual(q['syllabus_refs'],['1.1']);self.assertEqual(q['tag_ids'],['k1']);self.assertEqual(q['year'],2025);self.assertEqual(q['paper_type'],'paper2');self.assertEqual(q['legacy_metadata']['tags'][0]['tag_id'],'k1')
   r=m.migrate(args);self.assertEqual(r['inserted'],0);self.assertEqual(r['skipped_existing'],1);self.assertEqual(m.digest(src),before)
   args.dest=str(base/'withpdf')
   try:import pymupdf as fitz
   except ImportError:self.skipTest('Optional PDF extraction unavailable')
   doc=fitz.open();p=doc.new_page();p.insert_text((72,72),'Question 1. Adjacent question 2.');doc.save(root/'qp.pdf');doc.close();(root/'ms.pdf').write_bytes((root/'qp.pdf').read_bytes())
   r=m.migrate(args);self.assertEqual(len(r['files']),1);self.assertEqual(len(list((Path(args.dest)/'files').glob('*.pdf'))),1)
   with sqlite3.connect(Path(args.dest)/'study.sqlite') as c:q=json.loads(c.execute('select data from questions').fetchone()[0]);self.assertIn('Adjacent',q['text']);self.assertEqual(q['qp_file_id'],q['ms_file_id']);self.assertEqual(q['text_scope'],'whole_page_includes_adjacent_content')
   self.assertIsNone(m.locate(root,'../legacy.sqlite','9702'));self.assertEqual(m.digest(src),before)
 def test_corrupt_text_encoding(self):
  self.assertTrue(m.corrupt_text_encoding('Question'+chr(5)*20))
  self.assertFalse(m.corrupt_text_encoding('Question 1\n\tGood English. 中文。'))
 def test_tag_strings_and_paper_filters(self):
  fields=m.tag_fields([{'knowledge_point_cn':'  加速度 ','tag_id':'k1','syllabus_ref':'2.1'},{'knowledge_point_cn':'加速度','tag_id':'k1','syllabus_ref':'2.1'},{'knowledge_point_en':'Velocity','tag_id':'k2','syllabus_ref':'2.2'}])
  self.assertEqual(fields,{'topics':['加速度','Velocity'],'tag_ids':['k1','k2'],'syllabus_refs':['2.1','2.2']})
  self.assertEqual(m.paper_fields({'paper_id':'9702_m25_22','year':2025}),{'year':2025,'paper_type':'paper2'})
  self.assertEqual(m.paper_fields({'component':'nonsense'})['paper_type'],None)
 def test_missing_qp_pages_rejected_by_export(self):
  sys.path.insert(0,str(Path(__file__).parents[1]/'tools'))
  from export_packet import normalized_questions,IntakeError
  for pages in ([],None,[0]):
   with self.assertRaises(IntakeError):normalized_questions({'mode':'practice','questions':[{'id':'9702:1','source':'paper','question_no':'1','qp_path':'test.pdf','qp_pages':pages}]})
if __name__=='__main__':unittest.main()
