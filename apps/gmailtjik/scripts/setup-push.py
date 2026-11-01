#!/usr/bin/env python3
"""Provision an account's Gmail Pub/Sub pull subscription; secrets stay private."""
import argparse,base64,hashlib,http.server,json,os,secrets,socket,time,urllib.parse,urllib.request,urllib.error,webbrowser
from pathlib import Path


def private_write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
    temp=path.with_name('.'+path.name+'.'+secrets.token_hex(8))
    fd=os.open(temp,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
    with os.fdopen(fd,'w') as stream:json.dump(value,stream);stream.flush();os.fsync(stream.fileno())
    temp.replace(path)


def refresh(credentials):
    form=urllib.parse.urlencode({key:credentials[key] for key in ('client_id','client_secret','refresh_token')}|{'grant_type':'refresh_token'}).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request('https://oauth2.googleapis.com/token',data=form),timeout=30) as response:return json.load(response)['access_token']
    except urllib.error.HTTPError as error:raise RuntimeError('Cloud sign-in expired or unavailable; rerun with --authorize') from error


def authorize(mail):
    state=secrets.token_urlsafe(32);verifier=secrets.token_urlsafe(48)
    result={}
    class Callback(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            query=urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
            if query.get('state')!=[state]:self.send_error(400);return
            result.update(query);self.send_response(200);self.end_headers();self.wfile.write(b'Google authorization received. You can return to Gmailtjik.')
        def log_message(self,*args):pass
    server=http.server.HTTPServer(('127.0.0.1',0),Callback);server.timeout=1
    redirect=f'http://127.0.0.1:{server.server_port}/'
    query={'client_id':mail['client_id'],'redirect_uri':redirect,'response_type':'code','scope':'https://www.googleapis.com/auth/cloud-platform','access_type':'offline','prompt':'consent','state':state,'code_challenge_method':'S256','code_challenge':base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip('=')}
    url='https://accounts.google.com/o/oauth2/v2/auth?'+urllib.parse.urlencode(query)
    print('Complete Google Cloud consent in the browser.',flush=True)
    if not webbrowser.open(url):print(url,flush=True)
    until=time.monotonic()+600
    try:
        while not result and time.monotonic()<until:server.handle_request()
    finally:server.server_close()
    if not result.get('code'):raise RuntimeError('Google authorization was cancelled or timed out')
    form=urllib.parse.urlencode({'client_id':mail['client_id'],'client_secret':mail['client_secret'],'code':result['code'][0],'redirect_uri':redirect,'grant_type':'authorization_code','code_verifier':verifier}).encode()
    try:
        with urllib.request.urlopen(urllib.request.Request('https://oauth2.googleapis.com/token',data=form),timeout=30) as response:tokens=json.load(response)
    except urllib.error.HTTPError as error:raise RuntimeError('Google Cloud authorization exchange failed') from error
    if not tokens.get('refresh_token'):raise RuntimeError('Google did not grant offline Cloud access')
    return {'type':'authorized_user','client_id':mail['client_id'],'client_secret':mail['client_secret'],'refresh_token':tokens['refresh_token']}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data-dir',type=Path,default=Path(os.environ.get('GMAILTJIK_DATA_DIR',str(Path(os.environ.get('XDG_DATA_HOME',str(Path.home()/'.local/share')))/'gmailtjik'))))
    parser.add_argument('--authorize',action='store_true',help='Open a fresh Google Cloud consent flow')
    parser.add_argument('--credentials',type=Path,help='Existing authorized_user Cloud credentials; copied privately for this account')
    args=parser.parse_args();root=args.data_dir.expanduser().resolve()
    if any((p/'.git').is_file() or (p/'.git/HEAD').is_file() for p in [root,*root.parents]):raise RuntimeError('Account data must be outside a repository')
    mail=json.loads((root/'account.json').read_text());cloud_path=root/'pubsub-credentials.json'
    if args.authorize:
        credentials=authorize(mail);private_write(cloud_path,credentials)
    elif args.credentials:
        credentials=json.loads(args.credentials.read_text());refresh(credentials);private_write(cloud_path,credentials)
    elif cloud_path.exists():credentials=json.loads(cloud_path.read_text())
    else:raise RuntimeError('Use --authorize or --credentials to connect Google Cloud')
    token=refresh(credentials)
    def api(url,body=None,method=None,allow_missing=False):
        headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'}
        try:
            with urllib.request.urlopen(urllib.request.Request(url,data=json.dumps(body).encode() if body is not None else None,headers=headers,method=method),timeout=35) as response:
                data=response.read();return json.loads(data) if data else {}
        except urllib.error.HTTPError as error:
            if allow_missing and error.code==404:return None
            raise RuntimeError(f'Cloud setup request failed (HTTP {error.code}) at {urllib.parse.urlsplit(url).netloc}; verify project permissions and billing') from None
    # Gmail requires the topic to belong to the OAuth client's project.
    number=mail['client_id'].split('-')[0]
    if not number.isdigit():raise RuntimeError('Cannot resolve the Gmail OAuth project number')
    project=api('https://cloudresourcemanager.googleapis.com/v1/projects/'+number)['projectId']
    print('Configuring Gmail push in OAuth project: '+project,flush=True)
    service=f'https://serviceusage.googleapis.com/v1/projects/{number}/services/pubsub.googleapis.com'
    if api(service).get('state')!='ENABLED':
        operation=api(service+':enable',{})
        for _ in range(30):
            if operation.get('done'):break
            time.sleep(2);operation=api('https://serviceusage.googleapis.com/v1/'+operation['name'])
        if not operation.get('done') or operation.get('error'):raise RuntimeError('Pub/Sub API enablement did not complete')
    topic=f'projects/{project}/topics/gmailtjik-mail'
    if api('https://pubsub.googleapis.com/v1/'+topic,allow_missing=True) is None:api('https://pubsub.googleapis.com/v1/'+topic,{},'PUT')
    policy=api('https://pubsub.googleapis.com/v1/'+topic+':getIamPolicy?options.requestedPolicyVersion=3')
    principal='serviceAccount:gmail-api-push@system.gserviceaccount.com'
    bindings=policy.setdefault('bindings',[])
    binding=next((b for b in bindings if b.get('role')=='roles/pubsub.publisher' and not b.get('condition')),None)
    if binding is None:binding={'role':'roles/pubsub.publisher','members':[]};bindings.append(binding)
    if principal not in binding['members']:
        binding['members'].append(principal);api('https://pubsub.googleapis.com/v1/'+topic+':setIamPolicy',{'policy':policy})
    suffix=hashlib.sha256((str(root)+'@'+socket.gethostname()).encode()).hexdigest()[:16]
    subscription=f'projects/{project}/subscriptions/gmailtjik-{suffix}'
    existing=api('https://pubsub.googleapis.com/v1/'+subscription,allow_missing=True)
    if existing is None:api('https://pubsub.googleapis.com/v1/'+subscription,{'topic':topic,'ackDeadlineSeconds':180,'messageRetentionDuration':'604800s','expirationPolicy':{}},'PUT')
    elif existing.get('topic')!=topic or existing.get('pushConfig',{}).get('pushEndpoint'):raise RuntimeError('Existing subscription is not the expected Gmail pull subscription')
    private_write(root/'push.json',{'enabled':True,'topic':topic,'subscription':subscription,'credentials_file':str(cloud_path)})
    print('Push configured. The running Gmailtjik sync service will load it automatically.',flush=True)

if __name__=='__main__':
    try:main()
    except Exception as error:raise SystemExit(str(error))
