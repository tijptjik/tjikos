from pathlib import Path
import tempfile
import subprocess
import os
with tempfile.TemporaryDirectory() as tmp:
    root=Path(tmp)
    (root/'bin').mkdir()
    (root/'bin/runtjik').write_bytes((Path(__file__).resolve().parents[1] / 'bin/runtjik').read_bytes())
    (root/'bin/runtjik').chmod(0o755)
    (root/'runtjik-core').write_text('#!/bin/sh\nexit 99\n'); (root/'runtjik-core').chmod(0o755)
    (root/'hyprctl').write_text('#!/bin/sh\necho "$*" >> "$CLOSE_LOG"\n'); (root/'hyprctl').chmod(0o755)
    for decision, expected, closed in [('true',0,False),('false',0,True),('garbage',1,False),('',7,False)]:
        log=root/'close.log'; log.unlink(missing_ok=True)
        (root/'runtjik-gtk').write_text(f'#!/bin/sh\necho "{decision}"\nexit {7 if not decision else 0}\n'); (root/'runtjik-gtk').chmod(0o755)
        result=subprocess.run([str(root/'bin/runtjik'),'close-active'],env={**os.environ,'HOME':str(root),'PATH':str(root)+':'+os.environ['PATH'],'CLOSE_LOG':str(log)},capture_output=True)
        assert result.returncode==expected,(decision,result)
        assert log.exists()==closed,decision
        if closed: assert log.read_text()=='eval hl.dispatch(hl.dsp.window.close())\n'
print('Close wrapper: consumed, fallback, malformed response and failed control passed')
