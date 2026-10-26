#!/usr/bin/env python3
"""Private JSON bridge for mailtjik's two terminal modes (not a public CLI)."""
from __future__ import annotations
import base64
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from contextlib import redirect_stdout, contextmanager
import email
import email.policy
from email.message import EmailMessage
import hashlib
import fcntl
import io
import json
import os
import re
from pathlib import Path
import shutil
import sqlite3
import struct
import subprocess
import sys
import time
import traceback
from types import SimpleNamespace
import uuid
import zlib

import email_archive_index as index
import email_archive_live as live
from email_archive_annual import Store, ROOT, connect, sha, private_directory
from email_archive_run import save_json
from email_archives import partition_db, retrieve, catalogue_fetch, start_worker, terminal_text

DEFAULT_CONFIG = Path.home() / '.config/mailtjik/config.json'


def configuration(request):
    path = Path(request.get('config') or DEFAULT_CONFIG).expanduser()
    supplied = json.loads(path.read_text()) if path.exists() else {}
    defaults = {
        'workdir': str(Path.home() / '.local/state/mailtjik/archive'),
        'import_workdir': str(Path.home() / '.local/state/mailtjik/import'),
        'live_workdir': str(Path.home() / '.local/state/mailtjik/live'),
        'policy': str(Path.home() / '.config/document-archive-policy.json'),
        'remote': ROOT, 'source_config': '', 'connection': '', 'client': '', 'base_ledger': '',
    }
    defaults.update(supplied)
    for key in ('workdir','import_workdir','live_workdir','policy','source_config','connection','client','base_ledger'):
        if defaults[key]:
            defaults[key] = str(Path(defaults[key]).expanduser())
    return path, defaults


def safe(value):
    return ''.join(c for c in str(value or '') if c in '\n\t' or (c.isprintable() and not 0x202A <= ord(c) <= 0x202E))


def store_for(config):
    policy = json.loads(Path(config['policy']).read_text())
    return Store(Path(config['workdir']), policy['recipient'], policy['identity'], config['remote'])


def catalogue(config):
    path = Path(config['workdir']) / 'catalogue.json'
    if (path.parent / 'job.json').exists() and not (path.parent / 'published.json').exists():
        return {'partitions': []}
    if not path.exists():
        return {'partitions': []}
    result = json.loads(path.read_text())
    if result.get('version') != 2:
        raise ValueError('Unsupported archive catalogue version')
    return result


def status(config):
    result = {}
    for key in ('workdir','import_workdir','live_workdir'):
        path = Path(config[key]) / 'status.json'
        if path.exists():
            result[key] = json.loads(path.read_text())
    return result


def overview(config, demo=False):
    cat = demo_catalogue() if demo else catalogue(config)
    accounts, years = Counter(), Counter()
    for p in cat['partitions']:
        accounts[p['account']] += p['messages']
        years[p['year']] += p['messages']
    return {'total': sum(accounts.values()), 'accounts': dict(sorted(accounts.items())),
            'years': dict(sorted(years.items(), reverse=True)), 'partitions': cat['partitions'],
            'status': {'workdir': {'stage': 'demo — synthetic mail only'}} if demo else status(config),
            'published': bool(cat['partitions']), 'config': config}


def eligible(part, filters):
    if filters.get('account') and part['account'] != filters['account']:
        return False
    start, end = filters.get('start', ''), filters.get('end', '')
    if part['year'] == 'unknown-date':
        return not start and not end
    return (not start or int(part['year']) >= int(start)) and (not end or int(part['year']) <= int(end))


def validate_filters(filters):
    for key in ('start','end'):
        val = filters.get(key, '')
        if val and (not str(val).isdigit() or not 1 <= int(val) <= 9999):
            raise ValueError('Years must be between 0001 and 9999')
    if filters.get('start') and filters.get('end') and int(filters['start']) > int(filters['end']):
        raise ValueError('Start year must be no later than end year')
    if filters.get('role', 'any') not in ('any','from','to','cc','bcc'):
        raise ValueError('Choose any, from, to, cc or bcc')


def where_clause(filters):
    clauses, params = [], []
    if filters.get('participant'):
        role = filters.get('role', 'any')
        fields = ('from','to','cc','bcc') if role == 'any' else (role,)
        clauses.append('(' + ' OR '.join(f'instr(casefold(coalesce({f}_header,\'\')),?)>0' for f in fields) + ')')
        params.extend([filters['participant'].casefold()] * len(fields))
    if filters.get('subject'):
        clauses.append("instr(casefold(coalesce(subject,'')),?)>0")
        params.append(filters['subject'].casefold())
    return (' WHERE ' + ' AND '.join(clauses) if clauses else ''), params


def query(config, request):
    filters = request.get('filters', {})
    validate_filters(filters)
    if request.get('demo'):
        return demo_query(filters, request.get('offset', 0))
    store = store_for(config)
    cat = catalogue(config)
    if not cat['partitions']:
        raise ValueError('No published catalogue yet. Archive mode shows preparation progress.')
    where, params = where_clause(filters)
    offset = max(0, int(request.get('offset', 0)))
    limit = 40
    hits, rows, facets = 0, [], []
    selected = [p for p in cat['partitions'] if eligible(p, filters)]
    def warm_index(part):
        db = partition_db(store, part)
        db.close()
    # A cold all-account search must not serialize dozens of Drive round trips.
    # Only selected indexes are fetched; message objects are opened on demand.
    with ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(warm_index, selected))
    for p in selected:
        db = partition_db(store, p)
        try:
            db.create_function('casefold', 1, lambda s: str(s).casefold(), deterministic=True)
            count = db.execute('SELECT count(*) FROM messages' + where, params).fetchone()[0]
            hits += count
            facets.append({'account': p['account'], 'year': p['year'], 'available': p['messages'], 'matches': count})
            for row in db.execute('SELECT sha256,subject,normalized_utc,from_header,to_header,cc_header,bcc_header,byte_size FROM messages' + where + ' ORDER BY normalized_utc DESC,sha256 LIMIT ?', [*params, offset + limit]):
                rows.append({**dict(row), 'account': p['account'], 'year': p['year']})
        finally:
            db.close()
    rows.sort(key=lambda r: (r['normalized_utc'] or r['year'], r['sha256']), reverse=True)
    return {'matches': hits, 'available': sum(p['messages'] for p in cat['partitions'] if eligible(p, filters)),
            'rows': rows[offset:offset+limit], 'facets': facets, 'offset': offset, 'limit': limit}


def raw_path(config, row):
    if not re.fullmatch(r'[0-9a-f]{64}', row['sha256']):
        raise ValueError('Invalid raw message hash')
    store = store_for(config)
    p = next((p for p in catalogue(config)['partitions'] if p['account'] == row['account'] and p['year'] == row['year']), None)
    if p is None:
        raise ValueError('Collection no longer occurs in the selected catalogue')
    path = private_directory(Path(config['workdir']) / 'reader-cache') / (row['sha256'] + '.eml')
    if path.exists():
        if sha(path.read_bytes()) != row['sha256']:
            raise ValueError('Cached raw message checksum mismatch; remove that cache file before retrying')
    else:
        db = partition_db(store, p)
        try:
            if not retrieve(store, db, row['sha256'], 'raw', path):
                raise ValueError('Raw message is not present in this collection')
        finally:
            db.close()
    return path


def decode_body(raw):
    msg = email.message_from_bytes(raw, policy=email.policy.default)
    body = msg.get_body(preferencelist=('html','plain'))
    text, html = '', ''
    if body:
        data = body.get_payload(decode=True) or b''
        try:
            text = data.decode(body.get_content_charset() or 'utf-8', 'replace')
        except LookupError:
            text = data.decode('utf-8', 'replace')
        if len(text) > 500_000:
            text = text[:500_000] + '\n\n[Display shortened; export EML for the complete message.]'
        if body.get_content_type() == 'text/html':
            html, text = text, ''
    images, attachments = [], []
    for part in msg.walk():
        if part.is_multipart():
            continue
        data = part.get_payload(decode=True) or b''
        inline = part.get_content_disposition() == 'inline' or bool(part.get('Content-ID'))
        if inline and part.get_content_type() in ('image/png','image/jpeg','image/gif','image/webp'):
            if len(data) <= 16 * 1024 * 1024 and len(images) < 20:
                images.append({'name': safe(part.get_filename() or 'Inline image'), 'cid': str(part.get('Content-ID','')).strip('<>'),
                               'data': base64.b64encode(data).decode()})
        elif part.get_filename() or part.get_content_disposition() == 'attachment':
            attachments.append({'name': safe(part.get_filename() or 'attachment.bin'), 'size': len(data), 'sha256': sha(data)})
    from email_archive_export import extract_attachments
    attachments = [{'name': safe(name), 'size': len(data), 'sha256': digest}
                   for digest, name, data in extract_attachments(msg)]
    return {'subject': safe(msg.get('Subject')), 'from': safe(msg.get('From')), 'to': safe(msg.get('To')),
            'cc': safe(msg.get('Cc')), 'bcc': safe(msg.get('Bcc')), 'date': safe(msg.get('Date')),
            'markdown': safe(text) or ('' if html else '*No displayable text body. Export the original EML to open it in a mail client.*'),
            'html': safe(html), 'images': images, 'attachments': attachments}


def message(config, request):
    raw = demo_raw(request['row']['sha256']) if request.get('demo') else raw_path(config, request['row']).read_bytes()
    return decode_body(raw)


def export_message(config, request):
    if request.get('demo'):
        return {'notice': 'Demo: EML and attachments stay synthetic; no personal archive is accessed.'}
    source = raw_path(config, request['row'])
    outdir = private_directory(Path(config['workdir']) / 'restored' / (time.strftime('%Y%m%dT%H%M%S') + '-' + uuid.uuid4().hex[:8]))
    if request.get('attachments'):
        from email_archive_export import extract_attachments
        count = 0
        for digest, name, data in extract_attachments(email.message_from_bytes(source.read_bytes(), policy=email.policy.default)):
            target = outdir / (digest[:12] + '-' + name)
            if not target.exists():
                with target.open('xb') as out:
                    out.write(data)
                count += 1
        return {'notice': f'{count} attachments saved to {outdir}'}
    target = outdir / source.name
    with target.open('xb') as out, source.open('rb') as data:
        shutil.copyfileobj(data, out)
    return {'notice': f'Original EML saved to {target}'}


def spawn(config, unit_kind, module, arguments, directory):
    directory = private_directory(Path(directory))
    code = private_directory(directory / ('code-' + uuid.uuid4().hex[:12]))
    for path in Path(__file__).parent.glob('*.py'):
        shutil.copyfile(path, code / path.name)
    unit = 'mailtjik-' + unit_kind + '-' + sha(str(directory).encode())[:10]
    subprocess.run(['systemd-run','--user','--collect','--unit',unit,'--property=UMask=0077','--property=Nice=10',
                    '--property=StandardOutput=append:' + str(directory/'worker.log'),
                    '--property=StandardError=append:' + str(directory/'worker.log'),
                    sys.executable,str(code/module),*arguments], check=True, capture_output=True)
    return {'notice': 'Worker started; safe to close the TUI. ' + unit}


@contextmanager
def stable_inventory(config):
    directory = Path(config['live_workdir'])
    if not directory.exists():
        raise ValueError('Inventory a connected mailbox first')
    with (directory / 'live.lock').open('a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_SH | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError('Live inventory or capture is running; wait for its completion before selecting mail')
        yield


def archive_action(config, request):
    action = request['action']
    if request.get('demo'):
        if action == 'tree':
            folder = private_directory(Path('/tmp') / ('mailtjik-demo-' + str(os.getuid())))
            inv = {'version': 1, 'mode':'live', 'inventory_id':'demo', 'connection':'demo', 'partitions': [
                {**p, 'labels':[{'id':'news','name':'Subscriptions/Newsletters'},{'id':'updates','name':'Subscriptions/Product updates'}]}
                for p in demo_catalogue()['partitions']]}
            return {'inventory':inv,'output':str(folder / (uuid.uuid4().hex+'.json'))}
        if action == 'preview':
            plan = json.loads(Path(request['plan']).read_text())
            selections = {(p['year'],p['account']):set(p['exclude_label_ids']) for p in plan['selections']}
            count = sum(1 for r in demo_records() if (r['year'],r['account']) in selections and not set(r['labels']) & selections[(r['year'],r['account'])])
            return {'selected_provider_ids':count,'estimated_bytes':count*2400,'notice':'Synthetic preview. Nothing will be uploaded.'}
        return {'notice':'Demo mode: this action would start a durable archive worker.'}
    if action == 'prepare':
        require(config, 'source_config')
        return spawn(config,'prepare','mailtjik_prepare.py',['--workdir',config['import_workdir'],'--config',config['source_config']],config['import_workdir'])
    if action == 'export':
        require(config,'source_config','policy')
        args = SimpleNamespace(workdir=Path(config['workdir']),checkpoint=Path(config['import_workdir'])/'index.complete.json',
             config=Path(config['source_config']),policy=Path(config['policy']),remote=config['remote'],
             base_ledger=Path(config['base_ledger']) if config['base_ledger'] else None,wait_checkpoint=True)
        with redirect_stdout(io.StringIO()):
            start_worker(args)
        return {'notice':'Annual export worker started; progress stays available after closing the TUI.'}
    if action == 'authenticate':
        require(config,'client')
        if not config['connection']:
            raise ValueError('Set a private connection file path in Setup')
        if Path(config['connection']).exists():
            raise ValueError('Connection file already exists; choose a new path to authorize another mailbox')
        private_directory(Path(config['connection']).parent)
        with redirect_stdout(io.StringIO()):
            live.oauth(Path(config['connection']),Path(config['client']))
        return {'notice':'Read-only Gmail authorization saved. You can inventory this mailbox now.'}
    if action == 'inventory':
        require(config,'source_config','connection')
        return spawn(config,'inventory','email_archive_live.py',['--workdir',config['live_workdir'],
                    '--connection',config['connection'],'inventory','--config',config['source_config']],config['live_workdir'])
    if action == 'tree':
        inv_path = Path(config['live_workdir'])/'inventory.json'
        if not inv_path.exists():
            raise ValueError('Inventory a connected mailbox before selecting years and excluded labels')
        with stable_inventory(config):
            return {'inventory':json.loads(inv_path.read_text()),'output':str(inv_path.parent/('selection-'+uuid.uuid4().hex+'.json'))}
    if action == 'preview':
        with stable_inventory(config):
            info = json.loads((Path(config['live_workdir'])/'inventory.json').read_text())
            plan = json.loads(Path(request['plan']).read_text())
            db = connect(Path(config['live_workdir']) / 'inventory.sqlite', True)
            try:
                rows = live.plan_rows(db,info,plan)
                return {'selected_provider_ids':len(rows),'estimated_bytes':sum(r['estimated_bytes'] for r in rows)}
            finally:
                db.close()
    if action == 'capture':
        require(config,'connection')
        return spawn(config,'capture','email_archive_live.py',['--workdir',config['live_workdir'],
                    '--connection',config['connection'],'capture','--plan',request['plan']],config['live_workdir'])
    if action == 'export_capture':
        marker = Path(config['live_workdir'])/'capture.complete.json'
        if not marker.exists():
            raise ValueError('Complete a live capture first')
        source = Path(json.loads(marker.read_text())['import_directory'])
        # Each capture is a new immutable generation; the current published
        # ledger remains the base. Preserve its configured reader catalogue.
        work = Path(config['workdir']).parent/('capture-archive-'+uuid.uuid4().hex[:12])
        base = Path(config['workdir'])/'ledger.sqlite'
        if not (Path(config['workdir'])/'published.json').exists():
            raise ValueError('Publish the base archive before appending a live capture')
        args = SimpleNamespace(workdir=work,checkpoint=source/'index.complete.json',config=source/'config.json',
            policy=Path(config['policy']),remote=config['remote'],base_ledger=base,wait_checkpoint=False)
        with redirect_stdout(io.StringIO()):
            start_worker(args)
        return {'notice':f'New annual generation running in {work}. Set Archive directory to it after publication.'}
    if action == 'refresh':
        root = Path(config['workdir'])
        if (root / 'job.json').exists() and not (root / 'published.json').exists():
            raise ValueError('This archive directory has an unfinished publication; use a separate reader cache directory')
        store = store_for(config)
        listing = json.loads(store.rclone('lsjson',store.remote+'/catalogues','--files-only',capture_output=True,text=True).stdout)
        names = sorted(x['Name'] for x in listing if '.json.' in x['Name'] and x['Name'].endswith('.zst.age'))
        if not names:
            raise ValueError('No published catalogue is available on Drive yet')
        args=SimpleNamespace(object='catalogues/'+names[-1],workdir=Path(config['workdir']))
        with redirect_stdout(io.StringIO()):
            catalogue_fetch(args,store)
        return {'notice':'Downloaded and authenticated the latest catalogue'}
    raise ValueError('Unknown archive action')


def require(config,*keys):
    for key in keys:
        if not config.get(key) or not Path(config[key]).is_file():
            raise ValueError('Set an existing '+key.replace('_',' ')+' file in Setup')


# These fixtures are generated, public examples; never sourced from personal mail.
def demo_records():
    result=[]
    for year in range(2022,2026):
        for account in ('alex@example.org','studio@example.org'):
            for number in range(1,13):
                participant = 'Morgan Lee <morgan@example.net>' if number % 3 else 'Riley Park <riley@example.net>'
                subject = ('Project Atlas — autumn field notes' if number%4==0 else
                           'Studio newsletter — new horizons' if number%3==0 else
                           'Project Atlas — planning session' if number%2==0 else 'Coffee and a catch-up')
                result.append({'sha256':sha(f'{year}/{account}/{number}'.encode()),'year':str(year),'account':account,
                    'normalized_utc':f'{year}-{number:02d}-12T09:30:00Z','subject':subject,'from_header':participant,
                    'to_header':account,'cc_header':'Team <team@example.org>' if number%2==0 else '',
                    'bcc_header':'Archive <archive@example.org>' if number%5==0 else '', 'byte_size':2400,
                    'labels':['news'] if number%3==0 else ['updates'] if number%5==0 else ['inbox']})
    return result


def demo_catalogue():
    counts=Counter((r['account'],r['year']) for r in demo_records())
    return {'version':2,'partitions':[{'account':a,'year':y,'messages':c} for (a,y),c in sorted(counts.items(),reverse=True)]}


def demo_query(filters,offset):
    selected=[r for r in demo_records() if eligible(r,filters)]
    def matches(r):
        roles=('from','to','cc','bcc') if filters.get('role','any')=='any' else (filters['role'],)
        participant=filters.get('participant','').casefold()
        return (not participant or any(participant in r[f+'_header'].casefold() for f in roles)) and filters.get('subject','').casefold() in r['subject'].casefold()
    rows=sorted([r for r in selected if matches(r)],key=lambda r:(r['normalized_utc'],r['sha256']),reverse=True)
    facets=[{**p,'available':p['messages'],'matches':sum(1 for r in rows if r['account']==p['account'] and r['year']==p['year'])} for p in demo_catalogue()['partitions'] if eligible(p,filters)]
    return {'matches':len(rows),'available':len(selected),'rows':rows[offset:offset+40],'facets':facets,'offset':offset,'limit':40}


def demo_png():
    width,height=640,180
    data=bytearray()
    for y in range(height):
        data.append(0)
        for x in range(width):
            ridge=80+int(25*__import__('math').sin(x/75))
            data.extend((25,110+int(y/4),123) if y>ridge else (33+int(x/16),44+int(y/5),77+int(y/3)))
    def chunk(kind,data):
        return struct.pack('!I',len(data))+kind+data+struct.pack('!I',zlib.crc32(kind+data)&0xffffffff)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',width,height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(bytes(data)))+chunk(b'IEND',b'')


def demo_raw(digest):
    row=next(r for r in demo_records() if r['sha256']==digest)
    msg=EmailMessage();msg['Subject']=row['subject'];msg['From']=row['from_header'];msg['To']=row['to_header']
    if row['cc_header']:msg['Cc']=row['cc_header']
    if row['bcc_header']:msg['Bcc']=row['bcc_header']
    from datetime import datetime
    from email.utils import format_datetime
    msg['Date']=format_datetime(datetime.fromisoformat(row['normalized_utc'].replace('Z','+00:00')));msg.set_content('Hello Alex,\n\nThe autumn field notes are ready.\n\nMorgan')
    msg.add_alternative('''<h1>Autumn field notes</h1><p>Hello Alex,</p><p>The latest <strong>Project Atlas</strong> notes are ready. A small collection of places, conversations and ideas to revisit.</p><h2>Next steps</h2><ul><li>Review the coastal route</li><li>Choose three photographs for the journal</li><li>Meet at the studio next Thursday</li></ul><p><em>Keep the originals. Make room for discovery.</em></p><img src="cid:coast" alt="Coastal study"><p>See you soon,<br>Morgan</p>''',subtype='html')
    msg.get_payload()[-1].add_related(demo_png(),maintype='image',subtype='png',cid='<coast>',filename='coastal-study.png',disposition='inline')
    return msg.as_bytes()


def main():
    os.umask(0o077)
    request=json.load(sys.stdin)
    path,config=configuration(request)
    op=request['op']
    if op=='overview':result=overview(config,request.get('demo',False))
    elif op=='query':result=query(config,request)
    elif op=='message':result=message(config,request)
    elif op=='export_message':result=export_message(config,request)
    elif op=='archive':result=archive_action(config,request)
    elif op=='save_config':
        if request.get('demo'):
            result={'notice':'Demo setup is not saved'}
        else:
            values=request['values']
            if set(values)-set(config):raise ValueError('Unknown setup field')
            private_directory(path.parent);config.update(values);save_json(path,config)
            result={'notice':'Private setup saved'}
    else:raise ValueError('Unknown operation')
    json.dump({'ok':True,'data':result},sys.stdout,ensure_ascii=True)


if __name__=='__main__':
    try:
        main()
    except Exception as exc:
        # Only intentional input errors are shown. Provider/decoder exceptions
        # may contain sensitive data; no raw traceback reaches terminal or Git.
        message=str(exc) if isinstance(exc,ValueError) else f'{type(exc).__name__}: operation did not complete; check private configuration and worker status'
        json.dump({'ok':False,'error':message},sys.stdout,ensure_ascii=True)
