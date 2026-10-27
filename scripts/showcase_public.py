"""Read-only public market/music capture snapshot; local-only connectivity state."""
import json,os
import datetime
from pathlib import Path
from .model import item
from .icons import icon
from . import connectivity,details
DATA=json.loads(Path(os.environ['BARTJIK_PUBLIC_DATA']).read_text())
COUNTRY='jp'
TAILNET=True


def items(provider,options):
    if provider=='workspaces':
        return [dict(item(str(n),str(n),None,actions={'leftClick':'workspace.activate'}),workspace=n) for n in (1,2,3,9)]
    if provider=='datetime':
        row=item('clock',datetime.datetime.now().strftime('%H:%M'),None)
        row['panel']=details.world_clock();return [row]
    if provider=='crypto':
        result=[]
        for pair,value in DATA['markets'].items():
            row=item(pair,format(value['price'],',.2f').rstrip('0').rstrip('.'),icon({'BTC':'bitcoin','SOL':'solana','AR':'arweave'}[pair.split('/')[0]]),kind='image',actions={'leftClick':'crypto.panel'})
            row['changes']=value['changes'];result.append(row)
        return result
    if provider=='playerctl':
        m=DATA['music'];row=item('capture-player',m['title'],'pause',kind='symbol')
        row['icon']['actions']={'leftClick':'player.toggle'}
        row['artist']={'value':m['artist'],'style':{},'actions':{}}
        row['track']={'value':m['title'],'style':{},'actions':{}}
        row['media']={'artist':m['artist'],'title':m['title'],'album':m['album'],'artwork':Path(m['artwork']).as_uri(),'playing':True,'canToggle':True,'canPrevious':False,'canNext':False,'player':'Public preview'}
        return [row]
    if provider=='mullvad':
        names={'jp':('Japan','Tokyo'),'nl':('Netherlands','Amsterdam'),'sg':('Singapore','Singapore')}
        locations={k:{'id':k,'name':v[0],'cities':[{'id':{'jp':'tyo','nl':'ams','sg':'sin'}[k],'name':v[1]}]} for k,v in names.items()}
        name,city=names[COUNTRY];row=item('demo',None,icon('flags/'+COUNTRY),kind='image')
        row['panel']=connectivity.mullvad_panel('connected',name+', '+city,locations,['jp','nl','sg']);return [row]
    if provider=='tailscale':
        row=item('demo',None,'vpn_lock',kind='symbol')
        row['panel']=connectivity.tailscale_panel({'BackendState':'Running' if TAILNET else 'Stopped','Self':{'HostName':'demo-desktop'},'TailscaleIPs':['100.64.0.1'],'Peer':{'laptop':{'HostName':'demo-laptop','Online':True,'TailscaleIPs':['100.64.0.2']},'phone':{'HostName':'demo-phone','Online':False,'TailscaleIPs':['100.64.0.3']}}})
        return [row]
    return None


def control(context):
    global COUNTRY,TAILNET
    if context.get('module')=='vpn' and context.get('action')=='country':COUNTRY=context['selection'];return True
    if context.get('module')=='tailnet' and context.get('action')=='toggle':TAILNET=not TAILNET;return True
    return False
