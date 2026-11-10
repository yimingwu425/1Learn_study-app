import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
import pymupdf as fitz

TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
from inspect_pdf import inspect, IntakeError

class PDFIntakeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / "sample.pdf"
    def tearDown(self):
        self.tmp.cleanup()
    def write(self, pages, rotation=0):
        d=fitz.open()
        for lines in pages:
            p=d.new_page()
            for x,y,t in lines:
                p.insert_text((x,y),t,fontsize=11)
            p.set_rotation(rotation)
        d.save(self.path); d.close()
    def test_native_headings_and_marks(self):
        self.write([[(50,80,"1 Define potential difference. [2]"),(50,150,"2 Calculate the current. [3]")]])
        r=inspect(self.path)
        self.assertEqual([x['question_no'] for x in r['candidates']],['1','2'])
        self.assertEqual([x['marks'] for x in r['candidates']],[2,3])
        self.assertTrue(all(x['needs_review'] for x in r['candidates']))
        self.assertEqual(r['status'],'needs_review')
    def test_rotation_coordinates(self):
        self.write([[(50,80,"1 State the SI unit of current. [1]")]],90)
        r=inspect(self.path)
        self.assertEqual(r['candidates'][0]['question_no'],'1')
        self.assertEqual(r['rotation_pages'],[1])
    def test_continuation_shared_page(self):
        self.write([[(50,80,"1 A cell supplies a circuit."),(50,130,"(a) Find the current. [2]")],
                    [(50,80,"(b) Explain your answer. [1]"),(50,150,"2 Describe a measurement. [2]")]])
        r=inspect(self.path)
        self.assertEqual(r['candidates'][0]['qp_pages'],[1,2])
        self.assertEqual(r['candidates'][1]['qp_pages'],[2])
        self.assertEqual(r['candidates'][0]['marks'],3)
    def test_page_number_not_question(self):
        self.write([[(280,820,"1"),(50,80,"INSTRUCTIONS"),(50,120,"Write in black ink.")]])
        r=inspect(self.path)
        self.assertEqual(r['candidates'][0]['detection'],'manual_page_review')
        self.assertEqual(r['candidates'][0]['question_no'],'page-1')
    def test_blank_scan_requires_review(self):
        self.write([[]])
        r=inspect(self.path)
        self.assertFalse(r['ocr_performed'])
        self.assertEqual(r['candidates'][0]['confidence'],'none')
        self.assertIn('OCR', ' '.join(r['warnings']))
    def test_scan_after_native_is_not_silently_added(self):
        self.write([[(50,80,"1 State a definition. [1]")],[]])
        r=inspect(self.path)
        self.assertEqual(r['candidates'][0]['qp_pages'],[1])
        self.assertEqual(r['candidates'][1]['question_no'],'page-2')
    def test_reject_encrypted(self):
        d=fitz.open();d.new_page();d.save(self.path,encryption=fitz.PDF_ENCRYPT_AES_256,owner_pw='owner',user_pw='reader');d.close()
        with self.assertRaisesRegex(IntakeError,'Encrypted'):
            inspect(self.path)
    def test_reject_too_many_pages(self):
        d=fitz.open()
        for i in range(501):d.new_page()
        d.save(self.path);d.close()
        with self.assertRaisesRegex(IntakeError,'500'):
            inspect(self.path)
    def test_bad_pdf_cli_json(self):
        self.path.write_bytes(b'%PDF-1.7\nno trailer')
        p=subprocess.run([sys.executable,str(TOOLS/'inspect_pdf.py'),str(self.path)],capture_output=True,text=True)
        self.assertEqual(p.returncode,2)
        self.assertEqual(json.loads(p.stdout)['status'],'failed')
    def test_size_limit(self):
        with self.path.open('wb') as f:
            f.write(b'%PDF-1.7');f.truncate(25*1024*1024+1)
        with self.assertRaisesRegex(IntakeError,'25 MiB'):
            inspect(self.path)
    def test_render_rotated_and_bounds(self):
        self.write([[(50,80,"1 Describe the circuit. [2]")]],90)
        dest=Path(self.tmp.name)/'preview.png'
        p=subprocess.run([sys.executable,str(TOOLS/'render_pdf_page.py'),str(self.path),'1',str(dest)],capture_output=True,text=True)
        self.assertEqual(p.returncode,0,p.stderr+p.stdout)
        r=json.loads(p.stdout)
        self.assertTrue(dest.is_file())
        self.assertGreater(r['width'],r['height'])
        p=subprocess.run([sys.executable,str(TOOLS/'render_pdf_page.py'),str(self.path),'2',str(dest)],capture_output=True,text=True)
        self.assertEqual(p.returncode,2)

    def test_corrupt_native_layer_requires_visual(self):
        from unittest.mock import patch
        self.write([[(50,80,'1 Readable placeholder')]])
        with patch('inspect_pdf.page_lines', return_value=([(70,50,'1 '+chr(1)*20+' text',85)],595,842)):
            result=inspect(self.path)
        self.assertEqual(result['native_text_quality'],'corrupt')
        self.assertTrue(result['needs_visual'])
        self.assertEqual(result['corrupt_text_pages'],[1])
        self.assertFalse(result['ocr_performed'])

if __name__=='__main__':unittest.main()
