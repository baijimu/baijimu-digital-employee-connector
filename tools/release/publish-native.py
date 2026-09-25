"""Immutable employee artifacts, followed by source-owner publication.

Already-uploaded bytes must match; reruns never overwrite tags or release assets.
If a signed rebuild differs, use the archived first-run artifacts for recovery.
"""
import hashlib,json,os,subprocess,sys,tempfile,time,urllib.error,urllib.parse,urllib.request,zipfile
from download_transport import transport_url, safe_url
from pathlib import Path
from artifact_contract import validate_artifacts
def run(args,**kw):
 r=subprocess.run(args,**kw)
 if r.returncode:raise RuntimeError('Release operation failed: '+str(args[0]))
def get(url):
 target=transport_url(url);started=time.monotonic()
 print('Download start: '+safe_url(target),flush=True)
 try:
  with urllib.request.urlopen(urllib.request.Request(target,headers={'User-Agent':'Baijimu release'}),timeout=60) as r:
   data=r.read()
  print(f'Download complete: {safe_url(target)} bytes={len(data)} elapsed={time.monotonic()-started:.1f}s',flush=True)
  return data
 except urllib.error.HTTPError as e:
  e.close()
  if e.code==404:
   print('Download absent (404): '+safe_url(target),flush=True)
   return None
  raise RuntimeError(f'Download failed: {safe_url(target)} HTTP {e.code}') from None
 except (OSError,urllib.error.URLError) as e:
  raise RuntimeError(f'Download failed: {safe_url(target)} elapsed={time.monotonic()-started:.1f}s ({type(e).__name__}: {e.reason if isinstance(e,urllib.error.URLError) else type(e).__name__})') from None
def github_release(repo,tag):
 result=subprocess.run(['gh','api','--include',f'repos/{repo}/releases/tags/{urllib.parse.quote(tag,safe="")}'],capture_output=True,text=True)
 headers,separator,body=result.stdout.replace('\r\n','\n').partition('\n\n')
 first=headers.splitlines()[0].split() if headers else []
 status=first[1] if len(first)>1 and first[0].startswith('HTTP/') else None
 if status=='404':
  # The tag endpoint omits draft releases, including drafts we just created.
  listing=subprocess.run(['gh','api','--paginate','--slurp',f'repos/{repo}/releases?per_page=100'],capture_output=True,text=True)
  if listing.returncode:raise RuntimeError('GitHub draft lookup failed; absence was not confirmed')
  matches=[release for page in json.loads(listing.stdout) for release in page if release.get('tag_name')==tag]
  if len(matches)>1:raise RuntimeError('Ambiguous GitHub release identity')
  return matches[0] if matches else None
 if result.returncode or status!='200' or not separator:raise RuntimeError('GitHub release lookup failed; absence was not confirmed')
 return json.loads(body)

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
 manifest=json.loads(Path('connector.json').read_text(encoding="utf-8"));version=manifest['version'];repo=manifest['source']['repo'];tag=manifest['source']['revision']
 if repo!=os.environ['GITHUB_REPOSITORY']:raise RuntimeError('Release repository mismatch')
 out=Path('release-output')
 commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
 rows=validate_artifacts(out,manifest,commit,os.environ.get('RECOVERY_RUN_ID') or os.environ['GITHUB_RUN_ID'])
 config=json.loads(Path('.github/release-tools.json').read_text(encoding="utf-8"))
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
  release=github_release(repo,tag)
  if release is None:
   run(['gh','release','create',tag,'--repo',repo,'--verify-tag','--draft','--title',manifest['name']+' '+version,'--notes','独立数字员工 Connector；签名制品与来源版本分开验证，市场提交仍需独立审核。'])
   release=github_release(repo,tag)
   if release is None:raise RuntimeError('Created GitHub release could not be read back')
  assets={asset['name'] for asset in release['assets']}
  files=[oss_path,*[out/r['name'] for r in rows],*[out/(r['name']+'.sha256') for r in rows]]
  for path in files:
   download=tmp/'existing';download.mkdir(exist_ok=True)
   if path.name in assets:
    run(['gh','release','download',tag,'--repo',repo,'--pattern',path.name,'--dir',str(download)],stdout=subprocess.DEVNULL)
    if (download/path.name).read_bytes()!=path.read_bytes():raise RuntimeError('Immutable GitHub asset differs: '+path.name)
   else:run(['gh','release','upload',tag,str(path),'--repo',repo])
  run(['gh','release','edit',tag,'--repo',repo,'--draft=false','--latest=false'])
  os.environ['BAIJIMU_CLI']=str(cli);os.environ['MARKET_PUBLICATION_STATUS_FILE']=str(tmp/'publication-status')
  run(['bash',str(Path(__file__).resolve().parent/'publish-market.sh'),version,'connector.json',str(oss_path)])
  status=(tmp/'publication-status').read_text(encoding="utf-8").strip()
  if status not in ['PENDING_REVIEW','PUBLISHED']:raise RuntimeError('Unexpected publication status')
  print('Source publication:',status)
if __name__=='__main__':
 try:main()
 except Exception as e:print(type(e).__name__+': '+str(e),file=sys.stderr);sys.exit(1)
