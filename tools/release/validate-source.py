import json, os, re, subprocess
from pathlib import Path
m=json.loads(Path('connector.json').read_text()); p=json.loads(Path('package.json').read_text()); c=json.loads(subprocess.check_output(['cargo','metadata','--no-deps','--format-version','1']))['packages'][0]
v=m['version']
assert re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)',v)
assert p['version']==c['version']==v
assert m['appId']=='digital-employee-connector'
assert m['source']=={'type':'github','repo':'baijimu/baijimu-digital-employee-connector','revision':'v'+v}
assert m['runtime']['command']==next(t['name'] for t in c['targets'] if 'bin' in t['kind'])
lock=json.loads(Path('package-lock.json').read_text());assert lock['version']==lock['packages']['']['version']==v
if os.environ.get('PUBLISH')=='true':
 assert os.environ['RELEASE_REF']=='v'+v
 subprocess.run(['git','fetch','--no-tags','origin','main'],check=True)
 sha=subprocess.check_output(['git','rev-parse','HEAD']).strip()
 assert subprocess.check_output(['git','rev-list','-n','1','v'+v]).strip()==sha
 subprocess.run(['git','merge-base','--is-ancestor',sha.decode(),'origin/main'],check=True)
print('Verified source identity',m['appId'],v)
