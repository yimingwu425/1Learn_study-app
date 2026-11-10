"""Synthetic HTTP regression. Never reads the user's question or note database."""
import json, os, subprocess, tempfile, time, unittest, urllib.request, urllib.error
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]

class StudyMemoryTest(unittest.TestCase):
 def test_partial_writes_preserve_unseen_drafts_and_failed_writes_are_atomic(self):
  with tempfile.TemporaryDirectory() as directory:
   ready = Path(directory) / 'ready'
   process = subprocess.Popen([str(ROOT / 'server/target/debug/study-core')], env={**os.environ, 'STUDY_ROOT':str(ROOT), 'STUDY_DATA_DIR':directory, 'STUDY_PORT':'0', 'STUDY_READY_FILE':str(ready)}, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
   try:
    for _ in range(100):
     if ready.exists(): break
     time.sleep(.05)
    endpoint = ready.read_text() + '/api/study-memory'
    def call(body=None):
     request = urllib.request.Request(endpoint, data=json.dumps(body).encode() if body is not None else None, method='PUT' if body is not None else 'GET', headers={'Content-Type':'application/json','X-Study-Client':'memory-regression'})
     with urllib.request.urlopen(request, timeout=10) as response: return json.load(response)
    call({'lastId':'q1','questions':{'q1':{'draft':'keep'},'q2':{'draft':'old'}}})
    call({'questions':{'q2':{'draft':'new'}}})
    expected={'lastId':'q1','questions':{'q1':{'draft':'keep'},'q2':{'draft':'new'}}}
    self.assertEqual(call()['memory'],expected)
    call({'questions':{}})
    self.assertEqual(call()['memory'],expected)
    for invalid in [{'questions':{'q3':None}},{'lastId':None,'questions':{}},{'questions':{'q3':{'draft':'x'*1048576}}}]:
     with self.assertRaises(urllib.error.HTTPError) as error: call(invalid)
     self.assertEqual(error.exception.code,400)
     self.assertEqual(call()['memory'],expected)
   finally:
    process.terminate(); process.communicate(timeout=5)
