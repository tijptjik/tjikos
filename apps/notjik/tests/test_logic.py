import json
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("bun"), "Bun required for pure QML JavaScript checks")
class LogicTests(unittest.TestCase):
    def test_telegram_focus_suppression(self):
        source = (ROOT / "qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
for (const appId of ["org.telegram.desktop", "org.telegram.desktop._account", "telegram-desktop", "telegram"]) {
    for (const notice of [{app:"Telegram Desktop"}, {desktopEntry:"org.telegram.desktop"},
                          {app:"Telegram via Notjik"}, {type:"telegram"}]) {
        assert.equal(suppressForFocus(notice, appId), true);
        assert.equal(suppressForFocus(notice, "firefox"), false);
        assert.equal(suppressForFocus(notice, ""), false);
    }
    assert.equal(suppressForFocus({app:"Other"}, appId), false);
}
assert.equal(telegramWindow("org.telegram.desktopfake"), false);
'''
        subprocess.run(["bun", "-e", source + assertions], check=True)

    def test_robot_draw_is_stable_and_all_entries_are_reachable(self):
        source = (ROOT / "qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
assert.equal(robotFiles.length, 88);
const seen = new Set();
for (let i = 0; i < 64; i++) {
    Math.random = () => (i + 0.5) / 64;
    const notice = {id:"one",received:123};
    assignRobot(notice, null);
    assert.equal(notice.robotIndex,i);
    seen.add(robotArtwork(notice));
    const updated = {id:"one",body:"Changed"};
    Math.random = () => {throw Error("Updates must not reroll");};
    assignRobot(updated, notice);
    assert.equal(robotArtwork(updated),robotArtwork(notice));
    assert.equal(robotArtwork(JSON.parse(JSON.stringify(notice))),robotArtwork(notice));
}
assert.equal(seen.size,64);
for (const urgency of ["critical", "low"]) {
    const start = urgency === "critical" ? 64 : 76;
    const paths = new Set();
    for (let i = 0; i < 12; i++) {
        Math.random = () => (i + 0.5) / 12;
        const notice = {id:"urgent", received:789, urgency};
        assignRobot(notice, {robotIndex:0, urgency:"normal"});
        assert.equal(notice.robotIndex,start+i);
        paths.add(robotArtwork(notice));
        const updated = {...notice,body:"Changed"};
        Math.random = () => {throw Error("Same urgency must not reroll");};
        assignRobot(updated,notice);
        assert.equal(robotArtwork(updated),robotArtwork(notice));
        assert.equal(robotArtwork(JSON.parse(JSON.stringify(notice))),robotArtwork(notice));
    }
    assert.equal(paths.size,12);
    for (const robotIndex of [undefined,0,-1,88,"76",null]) {
        const legacy = {id:"old",received:456,urgency,robotIndex};
        assert.ok(paths.has(robotArtwork(legacy)));
        assert.equal(robotArtwork(legacy),robotArtwork({...legacy}));
    }
}
for (const from of ["normal","critical","low"]) {
    for (const to of ["normal","critical","low"]) {
        Math.random = () => 0.5;
        const previous = {urgency:from};
        assignRobot(previous,null);
        const updated = {urgency:to};
        assignRobot(updated,previous);
        assert.ok(validRobotIndex(updated.robotIndex,updated));
        if (from === to) assert.equal(updated.robotIndex,previous.robotIndex);
        else assert.notEqual(updated.robotIndex,previous.robotIndex);
    }
}

for (const value of [undefined,-1,64,1.5,"3",null]) {
    const legacy={id:"old",received:456,robotIndex:value};
    assert.equal(robotArtwork(legacy),robotArtwork({...legacy}));
    assert.ok(robotArtwork(legacy).startsWith("icons/robots/robot-"));
}
'''
        result = subprocess.run(["bun", "-e", source + assertions], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)


    def test_native_action_hooks(self):
        source = (ROOT / "qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
const config = {actions:{nativeHooks:[{caller:"chatgpt",identifier:"default",command:["show-codex"]}]}};
const notice = {app:" ChatGPT "};
assert.deepEqual(nativeActionHooks(notice,{identifier:"default"},config),[["show-codex"]]);
assert.deepEqual(nativeActionHooks({desktopEntry:"chatgpt"},{identifier:"default"},config),[["show-codex"]]);
assert.deepEqual(nativeActionHooks(notice,{identifier:"reply"},config),[]);
assert.deepEqual(nativeActionHooks({app:"other"},{identifier:"default"},config),[]);
assert.deepEqual(nativeActionHooks({...notice,archived:true},{identifier:"default"},config),[]);
config.actions.blacklist=["ChatGPT"];
assert.deepEqual(nativeActionHooks(notice,{identifier:"default"},config),[]);
'''
        result = subprocess.run(["bun", "-e", source + assertions], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_history_filter(self):
        source = (ROOT / "qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
const notices = [
    {historyKey:"1", app:"Telegram Desktop", title:"Weekend plans", body:"Meet at Central / pier", group:"Friends"},
    {historyKey:"2", app:"Telegram Desktop", title:"Work", body:"Build is ready"},
    {historyKey:"3", app:"Mail", desktopEntry:"org.mail", title:"CENTRAL booking", body:"Confirmed 香港"},
    {historyKey:"4"}
];
const keys = query => filterHistory(notices, query).map(n => n.historyKey);
assert.equal(filterHistory(notices, "  "), notices);
assert.deepEqual(keys(" central "), ["1", "3"]);
assert.deepEqual(keys("TELEGRAM central"), ["1"]);
assert.deepEqual(keys("weekend"), ["1"]);
assert.deepEqual(keys("friends"), ["1"]);
assert.deepEqual(keys("org.mail"), ["3"]);
assert.deepEqual(keys("香港"), ["3"]);
assert.deepEqual(keys("/"), ["1"]);
assert.deepEqual(keys("[.*"), []);
assert.deepEqual(keys("nothing here"), []);
assert.equal(notices.length, 4);
const groups = historyGroups(filterHistory(notices, "central"), true);
assert.deepEqual(groups.map(g => g.notices.map(n => n.historyKey)), [["1"], ["3"]]);
'''
        result = subprocess.run(["bun", "-e", source + assertions], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_action_blacklist_and_live_changes(self):
        source = (ROOT / "qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
const actions = [{label:"Open", identifier:"default"}, {label:"Reply", identifier:"reply"}];
const notice = {app:"Telegram", desktopEntry:"org.telegram.desktop", actions};
const config = {actions:{blacklist:[" telegram "]}};
assert.deepEqual(actionsFor(notice, config), []);
assert.deepEqual(actionsFor({...notice, app:"Other"}, {actions:{blacklist:["ORG.TELEGRAM.DESKTOP"]}}), []);
assert.equal(actionsFor({...notice, app:"Telegram helper", desktopEntry:""}, config), actions);
assert.equal(actionsFor({app:"Mail", actions}, config), actions);
assert.equal(actionsFor(notice, {}), actions);
config.actions.blacklist = [];
assert.equal(actionsFor(notice, config), actions);
assert.deepEqual(actionsFor({...notice, archived:true}, config), []);
assert.deepEqual(actionsFor({}, config), []);
assert.equal(notice.actions, actions);
assert.equal(telegramChatAction({...notice,id:'dbus-42'}),0);
assert.equal(telegramChatAction({...notice,id:'dbus-42',archived:true}),-1);
assert.equal(telegramChatAction({...notice,id:'local-42'}),-1);
assert.equal(telegramChatAction({...notice,id:'dbus-42',actions:[{identifier:'reply'}]}),-1);
assert.equal(telegramChatAction({...notice,id:'dbus-42',app:'Not Telegram',desktopEntry:''}),-1);
assert.deepEqual(actionsFor({...notice,id:'dbus-42'}, {actions:{blacklist:['telegram']}}),[]);

'''
        result = subprocess.run(["bun", "-e", source + assertions], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_blank_actions_do_not_create_buttons_or_shift_invocation(self):
        source = (ROOT / "qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
const actions = [
    {label:" ", identifier:"default"},
    {label:"Reply", identifier:"reply"},
    {label:"\t\n", identifier:"blank"},
    {label:"", identifier:"empty"},
    {label:"Open", identifier:"open"}
];
const notice = {app:"kitty", actions};
assert.deepEqual(actionButtonsFor({actions:[actions[0]]}, {}), []);
assert.deepEqual(actionButtonsFor(notice, {}).map(a=>[a.label,a.actionIndex]), [["Reply",1],["Open",4]]);
assert.equal(actionsFor(notice, {}), actions);
assert.equal(actions[0].identifier, "default");
assert.equal(actions[1].actionIndex, undefined);
assert.deepEqual(actionButtonsFor({...notice,archived:true}, {}), []);
assert.deepEqual(actionButtonsFor(notice, {actions:{blacklist:["kitty"]}}), []);
assert.deepEqual(actionButtonsFor({}, {}), []);
'''
        result = subprocess.run(["bun", "-e", source + assertions], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_geometry_timeout_and_action_validation(self):
        source = (ROOT / "qml/Logic.js").read_text()
        assertions = r'''
const assert = require("node:assert/strict");
assert.equal(spacedBody('A list\n• One\n• Two\nDone'), 'A list\n\n• One\n• Two\n\nDone');
assert.equal(spacedBody('• One\n\nDone'), '• One\n\nDone');
assert.equal(spacedBody('<b>literal</b>'), '<b>literal</b>');
const ink={text:'white',muted:'grey',critical:'pink'};
assert.equal(urgencyColors('low',ink).title,'grey');
assert.equal(urgencyColors('normal',ink).body,'grey');
assert.equal(urgencyColors('critical',ink).body,'white');
const p = {edge:"b", position:"c", margin:16, offsetX:0, offsetY:-24};
assert.deepEqual(geometry(p,1920,1080,420,200,1), {x:750,y:840});
assert.deepEqual(geometry({...p,edge:"t",position:"r",offsetX:"-10%",offsetY:"5%"},1000,800,200,100,1), {x:684,y:56});
assert.deepEqual(geometry({...p,edge:"l",position:"r",offsetY:0},1000,800,200,100,1), {x:16,y:684});
const primaryCfg={placement:{...p,monitors:"primary",primaryMonitor:"DP-1",perMonitor:{}}};
assert.equal(selected(primaryCfg,{name:"DP-2"},0,"DP-1"),false);
assert.equal(selected(primaryCfg,{name:"DP-1"},1,"DP-1"),true);
assert.equal(selected(primaryCfg,{name:"DP-2"},0,"DP-2"),true);
assert.equal(offset(24,1920,2),12);
assert.equal(offset("10%",1920,2),192);
const media={title:"Telegram",type:"telegram",data:{kind:"voice",source:"file:///tmp/voice.ogg",chatId:"123",messageId:"456",duration:8}};
validateNotice(media);
assert.throws(()=>validateNotice({...media,data:{...media.data,source:"https://example.com/remote.ogg"}}));
assert.throws(()=>validateNotice({...media,data:{...media.data,chatId:"someone"}}));
assert.throws(()=>validateNotice({...media,data:{...media.data,duration:-1}}));
assert.throws(()=>validateNotice({...media,data:{...media.data,kind:"executable"}}));
const cfg={placement:{...p,monitors:[2,"DP-1"],perMonitor:{"2":{edge:"t"},"DP-1":{edge:"r"}}},timeouts:{low:5,normal:8,critical:0}};
assert.equal(selected(cfg,{name:"HDMI-A-1"},0),false);
assert.equal(selected(cfg,{name:"HDMI-A-1"},1),true);
assert.equal(placement(cfg,{name:"DP-1"},1).edge,"r");
assert.equal(timeout(-1,"normal",cfg),8000);
assert.equal(timeout(0,"normal",cfg),0);
assert.equal(timeout(-1,"critical",cfg),0);
assert.equal(timeout(500,"critical",cfg),500);
assert.throws(()=>validateNotice({title:"a",actions:[{label:"x",command:"echo injected"}]}));
assert.throws(()=>validateNotice({title:"a",type:"progress",data:{value:101}}));
assert.throws(()=>validateNotice({title:"a",type:"timer",data:{seconds:-1}}));
validateNotice({title:"a",actions:[{label:"Open",command:["xdg-open","https://example.com/?a=1&b=2"]}]});
const groupedConfig={grouping:{enabled:true,windowSeconds:120},maxVisible:2};
const notices=[{id:"a",app:"mail",received:0},{id:"b",app:"build",received:1},{id:"c",app:"mail",received:2}];
assert.deepEqual(grouped(notices,groupedConfig,"").map(n=>n.id),["c","b"]);
assert.deepEqual(grouped(notices,groupedConfig,"")[0].memberIds,["a","c"]);
assert.equal(grouped(notices,groupedConfig,"a")[0].id,"a");
assert.equal(grouped(notices,{...groupedConfig,grouping:{enabled:false,windowSeconds:120}},"").length,3);
assert.equal(grouped([...notices,{id:"d",app:"mail",received:200000}],groupedConfig,"").length,3);
assert.equal(grouped([{id:"a",app:"x",group:"one",received:1},{id:"b",app:"x",group:"two",received:2}],groupedConfig,"").length,2);
assert.equal(visibleGroups(notices,{...groupedConfig,maxVisible:1},"b")[0].id,"b");
const burst=Array.from({length:6},(_,i)=>({id:String(i),app:String(i),received:i}));
assert.deepEqual(visibleGroups(burst,{...groupedConfig,maxVisible:10},"").map(n=>n.id),["3","2","1","0"]);
assert.deepEqual(visibleGroups(burst,{...groupedConfig,maxVisible:10},"0").map(n=>n.id),["3","2","1","0"]);
assert.deepEqual(visibleGroups(burst.slice(0,4),{...groupedConfig,maxVisible:4},"").map(n=>n.id),["3","2","1","0"]);
assert.deepEqual(visibleGroups(burst.slice(1),{...groupedConfig,maxVisible:4},"").map(n=>n.id),["4","3","2","1"]);
const updatedBurst=[...burst,{id:"update",app:"0",received:10}];
assert.deepEqual(visibleGroups(updatedBurst,{...groupedConfig,maxVisible:4},"").map(n=>n.groupId),["3","2","1","0"]);
assert.throws(()=>validateNotice({title:"test",group:3}));
validateNotice({title:"job",id:"backup/nightly"});
assert.throws(()=>validateNotice({title:"job",id:""}));
assert.throws(()=>validateNotice({title:"job",id:"bad key"}));
assert.throws(()=>validateNotice({title:"job",id:"x",replace:"local-1"}));
assert.equal(grouped([{id:"a",senderId:"one",app:"jobs",received:1},{id:"b",senderId:"two",app:"jobs",received:2}],groupedConfig,"").length,2);

'''
        result = subprocess.run(["bun", "-e", source + assertions], text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
