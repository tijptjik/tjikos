"""Exercise the PulseAudio compatibility transport with private executables only."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
APP = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='bartjik-audio-tools-') as directory:
    root = Path(directory)
    (root / 'bin').mkdir()
    def node(index, name, description, volume):
        return {'index': index, 'name': name, 'description': description, 'mute': False, 'volume': {'left': {'value': round(volume * 65536)}}, 'properties': {}}
    sinks = [node(1, 'usb', 'USB Audio Speakers', .4), node(2, 'screen', 'Navi 21/23 HDMI/DP Screen', .4)]
    sources = [node(3, 'alsa_input.usb-046d_Logitech_BRIO_demo', 'BRIO microphone', .65)]
    stream = node(4, 'music', 'Music', .7)
    stream.update({'client': 17, 'properties': {'application.name': 'Music', 'object.serial': '1001', 'application.process.id': '9999'}})
    state = root / 'state.json'
    state.write_text(json.dumps({'sinks': sinks, 'sources': sources, 'sink-inputs': [stream], 'default': 'usb'}))
    pactl = root / 'bin/pactl'
    pactl.write_text('#!' + sys.executable + '\n' + r'''
import json,os,sys,time
from pathlib import Path
p=Path(os.environ['BARTJIK_AUDIO_STATE']);state=json.loads(p.read_text());a=sys.argv[1:]
if a==['subscribe']:
    previous=p.read_text()
    print("Event 'change' on server #0",flush=True)
    while True:
        time.sleep(.01)
        current=p.read_text()
        if current!=previous:
            previous=current
            print("Event 'change' on server #0",flush=True)
elif a[:3]==['-f','json','list']:
    print(json.dumps(state[a[3]]))
elif a==['get-default-sink']:print(state['default'])
elif a==['get-default-source']:print(state['sources'][0]['name'])
elif len(a)==3 and a[0].startswith('set-'):
    kind,setting=a[0][4:].rsplit('-',1)
    collection={'sink':'sinks','source':'sources','sink-input':'sink-inputs'}[kind]
    item=next(v for v in state[collection] if v['index']==int(a[1]))
    if setting=='volume':item['volume']['left']['value']=round(float(a[2].removesuffix('%'))/100*65536)
    elif setting=='mute':item['mute']=a[2]=='1'
    else:raise RuntimeError('unexpected mutation')
    p.write_text(json.dumps(state))
else:raise RuntimeError('unexpected pactl command')
''')
    pactl.chmod(0o700)
    helper = root / 'bin/bartjik-gtk-core'
    helper.write_text('#!' + sys.executable + '\n' + r'''
import json,os,sys
from pathlib import Path
assert sys.argv[1:]==['helper','audio','toggle-output']
p=Path(os.environ['BARTJIK_AUDIO_STATE']);s=json.loads(p.read_text());s['default']='screen';p.write_text(json.dumps(s))
''')
    helper.chmod(0o700)
    env={**os.environ,'PATH':str(root/'bin')+os.pathsep+os.environ['PATH'],'BARTJIK_HOME':str(root),'BARTJIK_AUDIO_TEST_TOOLS':'1','BARTJIK_AUDIO_STATE':str(state),'PKG_CONFIG_PATH':str(APP/'../../packages/tjik-gtk/target/native/lib/pkgconfig')}
    # Build with the normal toolchain environment; isolate PATH only at runtime.
    build_env = {**os.environ, 'PKG_CONFIG_PATH': env['PKG_CONFIG_PATH']}
    build = subprocess.run(['cargo', 'test', '--release', '--locked', '--features', 'gtk-ui', '--bin', 'bartjik-gtk', '--no-run', '--message-format=json'], cwd=APP, env=build_env, capture_output=True, text=True, check=True)
    artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith('{')]
    binary = next(a['executable'] for a in artifacts if a.get('reason') == 'compiler-artifact' and a.get('executable') and a['target']['name'] == 'bartjik-gtk')
    subprocess.run([binary, 'audio_private_tools', '--ignored', '--nocapture'], cwd=APP, env=env, check=True)
print('PASS: synthetic pactl volume/mute, stale stream identity, bounds, and legacy route delegation')
