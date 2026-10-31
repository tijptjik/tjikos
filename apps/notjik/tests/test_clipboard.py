"""Markdown exports of the message body and every built-in card detail."""
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("bun"), "Bun required for QML JavaScript checks")
class ClipboardTests(unittest.TestCase):
    def test_card_exports(self):
        source = (ROOT / "qml/Clipboard.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
const copy = (type, data = {}, body = "", extra = {}) => markdown({
    type, data, body, title:"Title", received:1000, ...extra
}, 2000, null);
assert.equal(copy("standard", {}, "Saved /tmp/my_recording.mp4"), "Saved /tmp/my_recording.mp4");
assert.equal(copy("standard", {}, "First\r\n\r\nSecond"), "First\n\nSecond");
assert.equal(copy("standard"), "Title");
assert.equal(copy("status", {state:"Failed",detail:"Retry hourly"}, "Check logs"),
    "Check logs\n\n**Status:** Failed\n\nRetry hourly");
assert.equal(copy("progress", {value:74.6,label:"Transfer"}), "**Transfer:** 75%");
assert.equal(copy("progress", {value:0}), "**Progress:** 0%");
assert.equal(copy("timer", {seconds:65}), "**Remaining:** 01:04");
assert.equal(copy("timer", {seconds:1,doneLabel:"Finished"}), "**Finished:** 00:00");
assert.equal(copy("timer", {seconds:65}, "", {updatedAt:2000}), "**Remaining:** 01:05");
assert.equal(copy("results-table", {results:[{label:"Files",value:0,ingredients:[
    {label:"A|B",value:"one\ntwo"}]}],state:"Complete",duration:"42s"}),
    "| Field | Value |\n| --- | --- |\n| Files | 0 |\n| ↳ A\\|B | one<br>two |\n\n**Status:** Complete\n\n**Duration:** 42s");
assert.equal(copy("results-table"), "**Status:** Running");
assert.equal(copy("todo", {group:"Home",due:"tomorrow",taskId:"private-id"}, "Buy milk"),
    "Buy milk\n\n**Group:** Home\n\n**Due:** tomorrow");
assert.equal(copy("telegram", {kind:"text",styledText:"<b>Hello</b>"}, "Hello"), "Hello");
for (const [kind, label] of Object.entries({photo:"Photo",voice:"Voice message",sticker:"Sticker",emoji:"Animated emoji",animation:"Animation"})) {
    assert.equal(copy("telegram", {kind,source:"file:///private/media",chatId:42,messageId:1}), "**Media:** " + label);
}
assert.equal(copy("telegram", {kind:"voice",duration:62,transcript:"Hello world"}),
    "**Media:** Voice message (01:02)\n\n**Transcript:**\n\nHello world");
assert.equal(copy("telegram", {kind:"photo",caption:"A caption"}), "A caption\n\n**Media:** Photo");
assert.equal(copy("telegram", {kind:"sticker",fallback:"🐟"}), "**Media:** Sticker · 🐟");
assert.equal(copy("custom", {secret:"must not leak"}, "Custom body"), "Custom body");
const view = {type:"key-value",data:{heading:"A *heading*",fields:[{label:"Codec",value:"<x> & y"}]}};
assert.equal(markdown({title:"Title",body:"Raw input"},2000,view),
    "### A \\*heading\\*\n\n| Field | Value |\n| --- | --- |\n| Codec | &lt;x&gt; &amp; y | ".trim());
const longBody = "Long body\n".repeat(100);
assert.equal(copy("standard", {}, longBody, {groupCount:12,archived:true,actions:[{label:"Run",command:["secret"]}]}), longBody.trim());
'''
        subprocess.run(["bun", "-e", source + assertions], check=True)


if __name__ == "__main__":
    unittest.main()
