"""Own native build/signing route for the digital employee release unit."""
import base64, hashlib, json, os, shutil, subprocess, sys, tempfile, urllib.request, zipfile
from pathlib import Path

def run(args, **kw):
 r=subprocess.run(args,**kw)
 if r.returncode:raise RuntimeError('Release operation failed: '+str(args[0]))
def required(*names):
 for name in names:
  if not os.environ.get(name):raise RuntimeError('Missing release configuration: '+name)
def download(url,digest,path):
 urllib.request.urlretrieve(url,path)
 if hashlib.sha256(path.read_bytes()).hexdigest()!=digest:raise RuntimeError('Tool checksum mismatch')
def main():
 m=json.loads(Path('connector.json').read_text(encoding="utf-8"));binary=m['runtime']['command']
 platform=os.environ['RELEASE_PLATFORM'];arch=os.environ['RELEASE_ARCH'];publish=os.environ.get('PUBLISH')=='true'
 if platform=='windows':os.environ['RUSTFLAGS']='-C target-feature=+crt-static -D warnings'
 if platform=='macos':os.environ['MACOSX_DEPLOYMENT_TARGET']='11.0'
 run(['cargo','test','--locked'])
 with tempfile.TemporaryDirectory() as tmp:
  tmp=Path(tmp);root=tmp/'package';folder=root/'bin'/('macos' if platform=='macos' else platform+'-'+arch);folder.mkdir(parents=True)
  target=folder/(binary+('.exe' if platform=='windows' else ''))
  if platform=='macos':
   triples=['aarch64-apple-darwin','x86_64-apple-darwin'];run(['rustup','target','add',*triples])
   for triple in triples:run(['cargo','build','--release','--locked','--target',triple])
   run(['lipo','-create',*[f'target/{t}/release/{binary}' for t in triples],'-output',str(target)])
  else:
   run(['cargo','build','--release','--locked']);shutil.copy2(Path('target/release')/target.name,target)
  if publish and platform=='macos':
   required('APPLE_CERTIFICATE','APPLE_CERTIFICATE_PASSWORD','APPLE_SIGNING_IDENTITY')
   cert=tmp/'certificate.p12';cert.write_bytes(base64.b64decode(os.environ['APPLE_CERTIFICATE']));keychain=tmp/'signing.keychain-db';password=os.urandom(24).hex()
   try:
    run(['security','create-keychain','-p',password,str(keychain)])
    run(['security','unlock-keychain','-p',password,str(keychain)])
    run(['security','import',str(cert),'-k',str(keychain),'-P',os.environ['APPLE_CERTIFICATE_PASSWORD'],'-T','/usr/bin/codesign'])
    run(['security','set-key-partition-list','-S','apple-tool:,apple:,codesign:','-s','-k',password,str(keychain)])
    run(['codesign','--force','--timestamp','--options','runtime','--keychain',str(keychain),'--sign',os.environ['APPLE_SIGNING_IDENTITY'],str(target)])
    run(['codesign','--verify','--strict',str(target)])
   finally:subprocess.run(['security','delete-keychain',str(keychain)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  if publish and platform=='windows':
   required('SSL_COM_USERNAME','SSL_COM_PASSWORD','SSL_COM_CREDENTIAL_ID','SSL_COM_TOTP_SECRET')
   archive=tmp/'sign.zip';download('https://ssl.com/wp-content/uploads/2024/10/CodeSignTool-v1.3.1-windows.zip','e45a9e6c2aac4cae16c114eb590a2196406681357eb587507c65cd3646b5330d',archive)
   with zipfile.ZipFile(archive) as z:z.extractall(tmp/'sign')
   os.environ['CODESIGN_TOOL_PATH']=str(next((tmp/'sign').rglob('CodeSignTool.bat')))
   run(['pwsh','-NoProfile','-File','tools/release/sign-windows-artifact.ps1','-FilePath',str(target)])
  if subprocess.check_output([str(target),'--version']).decode().strip()!=m['version']:raise RuntimeError('Binary version mismatch')
  for name in ['connector.json','README.md','LICENSE']:shutil.copy2(name,root/name)
  out=Path('release-output');out.mkdir(exist_ok=True);name=m['source']['repo'].split('/')[-1]+'-'+m['version']+'-'+platform+'-'+arch+'.zip';archive=out/name
  with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as z:
   for f in sorted(root.rglob('*')):
    if f.is_file():z.write(f,f.relative_to(root))
  digest=hashlib.sha256(archive.read_bytes()).hexdigest();(out/(name+'.sha256')).write_text(digest+'  '+name+'\n')
  (out/(platform+'.json')).write_text(json.dumps({'platform':platform,'arch':arch,'name':name,'checksum':'sha256:'+digest}))
if __name__=='__main__':
 try:main()
 except Exception as e:print(type(e).__name__+': '+str(e),file=sys.stderr);sys.exit(1)
