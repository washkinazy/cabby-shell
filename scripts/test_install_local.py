"""Isolated install/upgrade/migration/uninstall; all manager calls are intercepted."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('install_local', ROOT/'scripts/install-local.py')
installer = importlib.util.module_from_spec(spec); spec.loader.exec_module(installer)

class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name);self.home=self.root/'home';self.home.mkdir()
        self.build=self.root/'build';self.build.mkdir();self.tools=self.root/'tools';self.tools.mkdir()
        self.env=os.environ|{'HOME':str(self.home),'XDG_DATA_HOME':str(self.home/'data'),'XDG_CONFIG_HOME':str(self.home/'config'),'PATH':str(self.tools),'INSTALL_TEST_LOG':str(self.root/'calls')}
        self.unit=self.home/'data/systemd/user';self.config=self.home/'config/systemd/user'
        for name in installer.BINARIES:
            p=self.build/name;p.write_text('#!/usr/bin/python3\nimport os,sys\nassert sys.argv[1:]==["validate"]\nsys.exit(int(os.environ.get("INVALID_CONFIG","0")))\n');p.chmod(0o755)
        p=self.tools/'systemctl';p.write_text('''#!/usr/bin/python3
import json,os,sys
with open(os.environ['INSTALL_TEST_LOG'],'a') as log:log.write(json.dumps(sys.argv[1:])+'\\n')
args=sys.argv[1:]
assert args[:1]==['--user']
assert args[1] in ['daemon-reload','show'], args
if args[1]=='show':print('active' if os.environ.get('ACTIVE_LOCK')=='1' and 'cabby-lock.service' in args else 'inactive')
''');p.chmod(0o755)
    def invoke(self, action='install', ok=True):
        result=subprocess.run(['/usr/bin/python3',str(ROOT/'scripts/install-local.py'),action,'--build-dir',str(self.build)],env=self.env,capture_output=True,text=True)
        self.assertEqual(result.returncode==0,ok,result.stdout+result.stderr);return result
    def test_install_upgrade_migration_overrides_and_uninstall(self):
        self.config.mkdir(parents=True)
        for name in installer.UNITS:
            (self.config/name).write_bytes((ROOT/'systemd/user'/name).read_bytes().replace(b'/usr/bin/cabby',b'%h/.local/bin/cabby'))
        dropin=self.config/'cabby-ui.service.d/personal.conf';dropin.parent.mkdir();dropin.write_text('[Service]\nEnvironment=PERSONAL=1\n')
        prefs=self.home/'.config/cabby-shell/config.toml';prefs.parent.mkdir(parents=True);prefs.write_text('font_size=12\n')
        self.invoke()
        for name in installer.UNITS:
            self.assertFalse((self.config/name).exists());self.assertIn(b'%h/.local/bin/cabby',(self.unit/name).read_bytes()) if name.endswith('.service') else None
        self.assertEqual((self.unit/'cabby-ui.service').stat().st_mode&0o777,0o644)
        self.assertEqual(prefs.read_text(),'font_size=12\n');self.assertTrue(dropin.exists())
        old=self.unit/'cabby-ui.service';old.write_text(old.read_text()+'# previous release\n')
        manifest=self.home/'data/cabby-shell/install.json';record=json.loads(manifest.read_text());record['files']['cabby-ui.service']=installer.digest(old);manifest.write_text(json.dumps(record))
        (self.build/'cabby').write_text((self.build/'cabby').read_text()+'# updated release\n');self.invoke()
        self.assertNotIn('previous release',(self.unit/'cabby-ui.service').read_text())
        self.assertIn('updated release',(self.home/'.local/bin/cabby').read_text())
        self.env['ACTIVE_LOCK']='1';self.invoke('uninstall',ok=False);self.assertTrue((self.home/'.local/bin/cabby').exists())
        self.env.pop('ACTIVE_LOCK');self.invoke('uninstall');self.assertFalse((self.unit/'cabby.target').exists());self.assertTrue(dropin.exists());self.assertTrue(prefs.exists())
    def test_modified_owned_file_blocks_upgrade_and_removal(self):
        self.invoke();unit=self.unit/'cabby-ui.service';unit.write_text(unit.read_text()+'# personal change\n')
        self.invoke(ok=False);self.invoke('uninstall',ok=False);self.assertIn('personal change',unit.read_text())
    def test_validation_precedes_mutation_and_custom_shadow_is_preserved(self):
        self.env['INVALID_CONFIG']='1';self.invoke(ok=False);self.assertFalse(self.unit.exists());self.assertFalse((self.home/'.local/bin/cabby').exists())
        self.env.pop('INVALID_CONFIG');self.config.mkdir(parents=True);custom=self.config/'cabby-ui.service';custom.write_text('[Service]\nExecStart=/usr/bin/true\n')
        self.invoke();self.assertEqual(custom.read_text(),'[Service]\nExecStart=/usr/bin/true\n')
    def test_unmanaged_vendor_collision_and_symlinks_are_preserved(self):
        self.unit.mkdir(parents=True);custom=self.unit/'cabby-ui.service';custom.write_text('custom\n');self.invoke(ok=False);self.assertEqual(custom.read_text(),'custom\n')
        custom.unlink();custom.symlink_to('/dev/null');self.invoke(ok=False);self.assertTrue(custom.is_symlink())
    def test_default_xdg_paths_and_unit_syntax(self):
        self.env.pop('XDG_DATA_HOME');self.env.pop('XDG_CONFIG_HOME');self.invoke()
        units=self.home/'.local/share/systemd/user';self.assertTrue((units/'cabby-lock.service').exists())
        # Verify staged units without loading/activating anything in the real manager.
        for p in units.iterdir():p.write_text(p.read_text().replace('%h/.local/bin/cabby','/usr/bin/true'))
        (units/'niri.service').write_text('[Service]\nExecStart=/usr/bin/true\n')
        result=subprocess.run(['systemd-analyze','--user','verify',*[str(p) for p in units.iterdir()]],capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)

if __name__=='__main__':unittest.main()
