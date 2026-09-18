"""Immutable employee artifacts, followed by source-owner publication.

Already-uploaded bytes must match; reruns never overwrite tags or release assets.
If a signed rebuild differs, use the archived first-run artifacts for recovery.
"""
import hashlib,json,os,subprocess,sys,tempfile,urllib.error,urllib.request,zipfile
from pathlib import Path
def run(args,**kw):
 r=subprocess.run(args,**kw)
 if r.returncode:raise RuntimeError('Release operation failed: '+str(args[0]))
def get(url):
 try:
  with urllib.request.urlopen(urllib.request.Request(url,headers={'User-Agent':'Baijimu release'}),timeout=60) as r:return r.read()
 except urllib.error.HTTPError as e:
  if e.code==404:return None
  raise

def immutable_upload(tool,path,key):
 url=os.environ['OSS_PUBLIC_BASE'].rstrip('/')+'/'+key
 existing=get(url);content=path.read_bytes()
 if existing is not None:
  if existing!=content:raise RuntimeError('Immutable OSS content differs: '+path.name)
 else:
  run([str(tool),'cp',str(path),'oss://'+os.environ['OSS_BUCKET']+'/'+key,'--access-key-id',os.environ['OSS_ACCESS_KEY_ID'],'--access-key-secret',os.environ['OSS_ACCESS_KEY_SECRET'],'--endpoint',os.environ['OSS_ENDPOINT'],'--region',os.environ['OSS_REGION'],'--no-progress'],stdout=subprocess.DEVNULL)
  if get(url)!=content:raise RuntimeError('OSS readback failed: '+path.name)
 return url

def main():
 names=['OSS_ACCESS_KEY_ID','OSS_ACCESS_KEY_SECRET','OSS_BUCKET','OSS_ENDPOINT','OSS_REGION','OSS_PUBLIC_BASE','LOCAL_APP_MARKET_PUBLISH_TOKEN','LOCAL_APP_OWNER_WORKSPACE_ID','GITHUB_REPOSITORY']
 for name in names:
  if not os.environ.get(name):raise RuntimeError('Missing release configuration: '+name)
 manifest=json.loads(Path('connector.json').read_text());version=manifest['version'];repo=manifest['source']['repo'];tag=manifest['source']['revision']
 if repo!=os.environ['GITHUB_REPOSITORY']:raise RuntimeError('Release repository mismatch')
 out=Path('release-output');rows=[json.loads((out/(p+'.json')).read_text()) for p in ['macos','windows','linux']]
 for row in rows:
  archive=out/row['name'];digest=hashlib.sha256(archive.read_bytes()).hexdigest()
  if row['checksum']!='sha256:'+digest:raise RuntimeError('Artifact digest mismatch')
  with zipfile.ZipFile(archive) as z:
   if json.loads(z.read('connector.json'))!=manifest:raise RuntimeError('Packaged manifest mismatch')
 config=json.loads(Path('.github/release-tools.json').read_text())
 with tempfile.TemporaryDirectory() as tmp:
  tmp=Path(tmp)
  for key in ['ossutil','baijimu']:
   archive=tmp/(key+'.zip');data=get(config[key]['url'])
   if data is None or hashlib.sha256(data).hexdigest()!=config[key]['sha256']:raise RuntimeError('Pinned tool checksum mismatch')
   archive.write_bytes(data)
   with zipfile.ZipFile(archive) as z:z.extractall(tmp/key)
  tool=next((tmp/'ossutil').rglob('ossutil'));tool.chmod(0o755)
  cli=next((tmp/'baijimu').rglob('baijimu'));cli.chmod(0o755)
  prefix='local-app-artifacts/'+manifest['appId']+'/releases/'+tag
  for row in rows:
   row['source']=immutable_upload(tool,out/row['name'],prefix+'/'+row['checksum'].split(':')[1]+'/'+row['name'])
   immutable_upload(tool,out/(row['name']+'.sha256'),prefix+'/'+row['checksum'].split(':')[1]+'/'+row['name']+'.sha256')
  oss={'schemaVersion':'2.0.0','appId':manifest['appId'],'releaseTag':tag,'version':version,'artifacts':rows}
  oss_path=out/(repo.split('/')[-1]+'-'+version+'-oss-manifest.json');oss_path.write_text(json.dumps(oss,sort_keys=True,indent=2)+'\n');immutable_upload(tool,oss_path,prefix+'/manifest.json')
  status=subprocess.run(['gh','release','view',tag,'--repo',repo,'--json','tagName,isDraft'],capture_output=True,text=True)
  if status.returncode:
   run(['gh','release','create',tag,'--repo',repo,'--verify-tag','--draft','--title',manifest['name']+' '+version,'--notes','独立数字员工 Connector；签名制品与来源版本分开验证，市场提交仍需独立审核。'])
  files=[oss_path,*[out/r['name'] for r in rows],*[out/(r['name']+'.sha256') for r in rows]]
  for path in files:
   download=tmp/'existing';download.mkdir(exist_ok=True)
   result=subprocess.run(['gh','release','download',tag,'--repo',repo,'--pattern',path.name,'--dir',str(download)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
   if result.returncode==0:
    if (download/path.name).read_bytes()!=path.read_bytes():raise RuntimeError('Immutable GitHub asset differs: '+path.name)
   else:run(['gh','release','upload',tag,str(path),'--repo',repo])
  run(['gh','release','edit',tag,'--repo',repo,'--draft=false','--latest=false'])
  os.environ['BAIJIMU_CLI']=str(cli);os.environ['MARKET_PUBLICATION_STATUS_FILE']=str(tmp/'publication-status')
  run(['bash','tools/release/publish-market.sh',version,'connector.json',str(oss_path)])
  status=(tmp/'publication-status').read_text().strip()
  if status not in ['PENDING_REVIEW','PUBLISHED']:raise RuntimeError('Unexpected publication status')
  print('Source publication:',status)
if __name__=='__main__':
 try:main()
 except Exception as e:print(type(e).__name__+': '+str(e),file=sys.stderr);sys.exit(1)
