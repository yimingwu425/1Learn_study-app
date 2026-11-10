import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile
import pymupdf as fitz

TOOLS = Path(__file__).resolve().parents[1] / 'tools'
sys.path.insert(0,str(TOOLS))
from export_packet import export_packet
from inspect_pdf import IntakeError

class ExportPacketTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)
        self.qp=self.root/'source-qp.pdf'; self.ms=self.root/'source-ms.pdf'
        for target,rotation in [(self.qp,90),(self.ms,0)]:
            d=fitz.open()
            for i in range(3):
                p=d.new_page();p.insert_text((50,80),f'Original source page {i+1}')
                p.draw_rect(fitz.Rect(50,100,200,170));p.set_rotation(rotation)
            d.save(target);d.close()
        self.output=self.root/'packet.zip';self.manifest=self.root/'manifest.json'
        self.question={'id':'q1','source':'Cambridge sample — 示例','question_no':'1(a)',
                       'qp_path':str(self.qp),'qp_pages':[1,2],
                       'ms_path':str(self.ms),'ms_pages':[2],'note':'My private explanation'}
    def tearDown(self):self.tmp.cleanup()
    def run_export(self,q=None,mode='practice'):
        self.manifest.write_text(json.dumps({'questions':[q or self.question],'mode':mode}),encoding='utf-8')
        return export_packet(self.manifest,self.output)
    def test_full_pages_rotation_and_provenance(self):
        before=hashlib.sha256(self.qp.read_bytes()).hexdigest()
        r=self.run_export()
        self.assertEqual(r['student_pages'],3);self.assertEqual(r['answer_pages'],2)
        with zipfile.ZipFile(self.output) as z:
            self.assertEqual(set(z.namelist()),{'student.pdf','answers.pdf','provenance.json','README.txt'})
            d=fitz.open(stream=z.read('student.pdf'),filetype='pdf')
            self.assertEqual(d[1].rotation,90)
            self.assertIn('Original source page 1',d[1].get_text())
            self.assertNotIn('My private explanation',''.join(p.get_text() for p in d))
            d.close()
            p=json.loads(z.read('provenance.json'))['questions'][0]
            self.assertEqual(p['qp_packet_pages'],[2,3]);self.assertEqual(p['ms_packet_pages'],[2])
            self.assertNotIn(str(self.root),z.read('provenance.json').decode())
        self.assertEqual(hashlib.sha256(self.qp.read_bytes()).hexdigest(),before)
    def test_study_includes_note(self):
        self.run_export(mode='study')
        with zipfile.ZipFile(self.output) as z:
            d=fitz.open(stream=z.read('student.pdf'),filetype='pdf')
            self.assertIn('My private explanation',d[0].get_text());d.close()
    def test_missing_answer_is_explicit(self):
        q=dict(self.question,ms_path=None,ms_pages=[])
        r=self.run_export(q)
        self.assertEqual(r['missing_answers'],['q1'])
        with zipfile.ZipFile(self.output) as z:
            self.assertNotIn('answers.pdf',z.namelist())
            self.assertIn('q1',z.read('README.txt').decode())
    def test_missing_qp_fails_no_output(self):
        q=dict(self.question,qp_path=str(self.root/'missing.pdf'))
        with self.assertRaises(IntakeError):self.run_export(q)
        self.assertFalse(self.output.exists())
    def test_missing_supplied_ms_fails(self):
        q=dict(self.question,ms_path=str(self.root/'missing.pdf'))
        with self.assertRaises(IntakeError):self.run_export(q)
        self.assertFalse(self.output.exists())
    def test_out_of_range_fails(self):
        for role in ['qp','ms']:
            q=dict(self.question);q[role+'_pages']=[4]
            with self.assertRaisesRegex(IntakeError,'outside'):self.run_export(q)
        self.assertFalse(self.output.exists())
    def test_duplicate_page_rejected(self):
        with self.assertRaisesRegex(IntakeError,'duplicate'):self.run_export(dict(self.question,qp_pages=[1,1]))
    def test_no_answer_path_with_answer_pages_rejected(self):
        with self.assertRaisesRegex(IntakeError,'no ms_path'):self.run_export(dict(self.question,ms_path=None))
    def test_bad_page_type_rejected(self):
        with self.assertRaisesRegex(IntakeError,'integer'):self.run_export(dict(self.question,qp_pages=[True]))
    def test_failure_does_not_replace_existing_output(self):
        self.output.write_bytes(b'existing artifact')
        with self.assertRaises(IntakeError):self.run_export(dict(self.question,qp_pages=[999]))
        self.assertEqual(self.output.read_bytes(),b'existing artifact')
    def test_cli_returns_json_error(self):
        self.manifest.write_text('{}')
        p=subprocess.run([sys.executable,str(TOOLS/'export_packet.py'),str(self.manifest),str(self.output)],capture_output=True,text=True)
        self.assertEqual(p.returncode,2)
        self.assertEqual(json.loads(p.stdout)['status'],'failed')

if __name__=='__main__':unittest.main()
