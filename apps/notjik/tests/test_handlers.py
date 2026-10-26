import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import notjik

RULE = json.loads((ROOT / "examples/handlers.json").read_text())["handlers"][0]
BODY = "📺 [Example] Sample Series - S02 v2 [WEB]\n\n1080p · WEB · x264\nGrabbed by Sonarr · Example.index"

class HandlerTests(unittest.TestCase):
    def test_configuration_rejects_invalid_templates(self):
        with tempfile.TemporaryDirectory() as temp, patch.dict(os.environ, {"NOTJIK_CONFIG":temp+"/config.json"}):
            path = Path(temp)/"config.json"
            path.write_text(json.dumps({"handlers":[RULE]}))
            self.assertEqual(notjik.load_config(ROOT)["handlers"], [RULE])
            invalid = [dict(RULE, match={"title":"StarProwlarr"}), dict(RULE, pattern="("),
                       dict(RULE, captures={"heading":99}), dict(RULE, card=[]),
                       dict(RULE, fields=[{"label":"Bad", "key":[]}]),
                       dict(RULE, captures={"constructor":1})]
            for rule in invalid:
                path.write_text(json.dumps({"handlers":[rule]}))
                with self.subTest(rule=rule), self.assertRaises(ValueError):
                    notjik.load_config(ROOT)

    @unittest.skipUnless(shutil.which("bun"), "Bun required")
    def test_matching_and_lossless_fallback_for_both_transports(self):
        source = (ROOT/"qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
const config = {handlers:[rule]};
const notice = {id:"one",type:"standard",app:"Telegram Desktop",title:"StarProwlarr",body,actions:[{label:"Open"}],data:{}};
const original = JSON.stringify(notice);
const parsed = notificationPresentation(notice,config);
assert.equal(parsed.type,"key-value");
assert.equal(parsed.data.heading,"[Example] Sample Series - S02 v2 [WEB]");
assert.equal(parsed.data.codec,"x264");
assert.equal(parsed.data.fields.length,6);
assert.equal(JSON.stringify(notice),original);
const bridge = {...notice,type:"telegram",app:"Telegram via Notjik",data:{kind:"text",chatId:"123",parts:[{kind:"text",text:body}]}};
assert.deepEqual(notificationPresentation(bridge,config),parsed);
assert.deepEqual(notificationPresentation({...notice,body:body.replace(/\n/g,"\r\n")},config),parsed);
for (const changed of [{title:"Other"},{app:"Mail"},{body:"New message"},{body:body+"\nExtra unexpected line"},{body:"1080p · WEB · x264"},{type:"progress"}])
 assert.equal(notificationPresentation({...notice,...changed},config),null);
assert.equal(notificationPresentation({...bridge,data:{kind:"photo"}},config),null);
assert.equal(notificationPresentation(notice,{handlers:[{...rule,pattern:"("}]}),null);
assert.equal(notificationPresentation(notice,{handlers:[{...rule,match:{telegram:true,chatId:"123"}}]}),null);
assert.ok(notificationPresentation(bridge,{handlers:[{...rule,match:{telegram:true,chatId:"123"}}]}));
assert.equal(notificationPresentation(notice,{handlers:[]}),null);
assert.equal(notificationPresentation(notice,{handlers:[{...rule,id:"first"},rule]}).handler,"first");
'''
        subprocess.run(["bun", "-e", source + "\nconst rule="+json.dumps(RULE)+";const body="+json.dumps(BODY)+";\n"+assertions],check=True)
