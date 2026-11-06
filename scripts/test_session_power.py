"""Session CLI requests with a PATH containing only an intercepted systemctl."""
import json,os,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class SessionPower(unittest.TestCase):
    def test_requests_are_guarded_literal_and_inhibitor_aware(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);helper=root/'systemctl'
            helper.write_text('''#!/usr/bin/python3
import json,sys
from pathlib import Path
p=Path(__file__).parent
args=sys.argv[1:]
with (p/'calls').open('a') as f:f.write(json.dumps(args)+'\\n')
if 'is-active' in args:sys.exit(int((p/'inactive').exists()))
if 'start' in args:sys.exit(int((p/'no-lock').exists()))
if args[-1] in ('reboot','poweroff','suspend'):sys.exit(int((p/'deny-power').exists()))
assert 'reset-failed' in args or args==['--no-ask-password','--user','--no-block','stop','niri.service'],args
''');helper.chmod(0o700)
            def request(action):
                (root/'calls').write_text('')
                p=subprocess.run([ROOT/'target/debug/cabby',action],env=os.environ|{'PATH':str(root)},capture_output=True,text=True,timeout=6)
                return p,[json.loads(v) for v in (root/'calls').read_text().splitlines()]
            for action in ('reboot','poweroff'):
                p,calls=request(action);self.assertEqual(p.returncode,0,p.stderr)
                self.assertEqual(calls,[['--no-ask-password','--user','is-active','--quiet','niri.service'],['--no-ask-password','--check-inhibitors=yes','--no-block',action]])
            p,calls=request('logout');self.assertEqual(p.returncode,0,p.stderr)
            self.assertEqual(calls[-1],['--no-ask-password','--user','--no-block','stop','niri.service'])
            (root/'inactive').touch()
            for action in ('logout','reboot','poweroff','suspend'):
                p,calls=request(action);self.assertNotEqual(p.returncode,0);self.assertEqual(len(calls),1)
            (root/'inactive').unlink();(root/'no-lock').touch()
            p,calls=request('suspend');self.assertNotEqual(p.returncode,0);self.assertFalse(any(v[-1]=='suspend' for v in calls))
            (root/'no-lock').unlink();(root/'deny-power').touch()
            for action in ('reboot','poweroff','suspend'):
                p,calls=request(action);self.assertNotEqual(p.returncode,0)
                self.assertFalse(any('stop' in v for v in calls),'rejected machine action stopped the session')
