import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const L=vm.createContext({});vm.runInContext(readFileSync(new URL('../qml/Logic.js',import.meta.url),'utf8'),L);
const base={side:'b',position:'l',alignment:'r',layout:{},padding:0,offsetX:0,offsetY:0};
let count=0;function test(name,f){f();count++;}
const near=(a,b)=>assert.ok(Math.abs(a-b)<1e-6,`${a} != ${b}`);
test('required 1920/640/400',()=>{const g=L.geometry(base,1920,1080,400,40,1);near(g.x,240);near(g.x+g.width,640);});
test('explicit 960px',()=>near(L.geometry({...base,layout:{centre_width:'960px'}},1920,1080,400,40,1).x,80));
for(const side of ['t','b','l','r'])for(const position of ['l','c','r'])for(const alignment of ['l','c','r'])for(const scale of [1,1.5,2]){
 test(`${side}/${position}/${alignment}@${scale}`,()=>{
  const h=['t','b'].includes(side), span=h?1920:1080;
  const b={...base,side,position,alignment,padding:12};
  const g=L.geometry(b,1920,1080,h?200:40,h?40:200,scale);
  const origin=span/3*(['l','c','r'].indexOf(position));
  const expected=origin+12+({'l':0,'c':.5,'r':1}[alignment])*(span/3-24-200);
  near(h?g.x:g.y,expected);
 });
}
for(const value of [640,'640px','40%'])for(const scale of [1,2])test(`span ${value} @${scale}`,()=>{
 const c=L.columns({centre_width:value},1920,scale);near(c.c.size,value==='40%'?768:640/scale);near(c.l.size,c.r.size);
});
test('offset pixels and percentages use full corresponding monitor',()=>{
 const g=L.geometry({...base,offsetX:'10%',offsetY:'-40px'},1920,1080,400,40,2);near(g.x,432);near(g.y,1020);
});
test('vertical centre span uses height',()=>{const g=L.geometry({...base,side:'r',layout:{centre_width:'50%'},position:'c'},1920,1080,40,200,1);near(g.columnSize,540);near(g.y,610);});
test('overflow clips in selected stable column',()=>{const g=L.geometry(base,1920,1080,900,40,1);near(g.width,640);near(g.x,0);assert.equal(g.overflow,true);});
test('empty other sections cannot move centre',()=>{for(const n of [0,100,10000])near(L.geometry({...base,position:'c',alignment:'c'},1920,1080,n,40,1).x+Math.min(n,640)/2,960);});
test('stack inward in all sides',()=>{for(const side of ['t','b','l','r']){const g=L.geometry({...base,side,stackOffset:48},1920,1080,40,40,1);near(['t','b'].includes(side)?g.y:g.x,{t:48,b:992,l:48,r:1832}[side]);}});
test('invalid widths rejected',()=>{for(const v of [-1,2000,'101%'])assert.throws(()=>L.columns({centre_width:v},1920,1));});
test('independent icon and text actions with per-item precedence',()=>{
 const row={id:'BTC/USD',actions:{leftClick:'module'},icon:{actions:{leftClick:'icon'}},text:{actions:{leftClick:'text'}}};
 const m={items:{'BTC/USD':{icon:{actions:{click:'override',rightClick:'right'}}}}};
 assert.equal(L.choose(m,row,'icon','click'),'override');assert.equal(L.choose(m,row,'text','leftClick'),'text');assert.equal(L.choose(m,row,'icon','rightClick'),'right');
 for(const name of ['middleClick','scrollUp','scrollDown','hoverEnter','hoverLeave','doubleClick'])assert.equal(L.choose({actions:{[name]:name}},row,'text',name),name);
});
test('double click cancels pending single',()=>{const state={};assert.equal(L.click(state,'leftClick',true).delay,true);assert.equal(L.click(state,'doubleClick',true).cancel,true);assert.equal(state.pending,false);assert.equal(L.click(state,'leftClick',false).fire,'leftClick');});
test('hover transitions once',()=>{const state={inside:false};assert.equal(L.transition(state,true),'hoverEnter');assert.equal(L.transition(state,true),null);assert.equal(L.transition(state,false),'hoverLeave');assert.equal(L.transition(state,false),null);});
test('stable keyed delegates move without clearing',()=>{
 const a=[{key:'BTC',token:1},{key:'SOL',token:2}];const model={get count(){return a.length;},get:i=>a[i],remove:i=>a.splice(i,1),insert:(i,v)=>a.splice(i,0,v),move:(f,t)=>a.splice(t,0,a.splice(f,1)[0])};
 L.sync(model,['SOL','BTC','AR']);assert.equal(a[0].token,2);assert.equal(a[1].token,1);L.sync(model,['BTC','AR']);assert.equal(a[0].token,1);
});
const screen={name:'DP-2',width:1920,height:1080,scale:1};
const edgeBar={autoHide:true,side:'b',activationEdge:'side',activationDelay:500,hideDelay:1000,keepVisibleDistance:'420px',revealSize:3};
const point=(y,monitor='DP-2')=>({valid:true,monitor,x:100,y});
const step=(s,p,t,b=edgeBar,h=false)=>L.edgeState(s,b,p,screen,h,t);
test('edge threshold continuous dwell and cancellation',()=>{
 let s={concealed:true};s=step(s,point(1079),0);s=step(s,point(1079),499);assert.equal(s.concealed,true);
 s=step(s,point(1000),500);s=step(s,point(1079),600);assert.equal(step(s,point(1079),1099).concealed,true);assert.equal(step(s,point(1079),1100).concealed,false);
});
test('safe travel across 300px gap, hide delay and reentry',()=>{
 let s=step({concealed:false},point(740),0);assert.equal(s.concealed,false);
 s=step(s,point(500),10);assert.equal(step(s,point(500),1009).concealed,false);
 s=step(s,point(740),900);s=step(s,point(500),1000);assert.equal(step(s,point(500),1999).concealed,false);assert.equal(step(s,point(500),2000).concealed,true);
});
test('zero delays, monitor isolation, hover/menu protection and failure',()=>{
 const b={...edgeBar,activationDelay:0,hideDelay:0};
 assert.equal(step({concealed:true},point(1079),0,b).concealed,false);
 assert.equal(step({},point(100),0,b).concealed,true);
 assert.equal(step({concealed:true},point(1079,'DP-1'),0,b).concealed,true);
 assert.equal(step({},point(100),0,b,true).concealed,false);
 assert.equal(step({concealed:true},{valid:false},0,b).concealed,false);
 assert.equal(step({concealed:true},point(100),0,{...b,autoHide:false}).concealed,false);
});
for(const edge of ['t','b','l','r'])test('edge orientation '+edge,()=>{
 const b={...edgeBar,activationEdge:edge,activationDelay:0,hideDelay:0,keepVisibleDistance:'10%'};
 const p={valid:true,monitor:'DP-2',x:edge==='r'?1919:1,y:edge==='b'?1079:1};
 assert.equal(step({concealed:true},p,0,b).concealed,false);
 assert.equal(step({}, {...p,x:960,y:540},0,b).concealed,true);
});
test('physical safe distance at scale2 and percent at scale2',()=>{
 const sc={...screen,scale:2}, b={...edgeBar,hideDelay:0,keepVisibleDistance:'400px'};
 assert.equal(L.edgeState({},b,point(879),sc,false,0).concealed,true);
 assert.equal(L.edgeState({},b,point(880),sc,false,0).concealed,false);
 assert.equal(L.edgeState({},{...b,keepVisibleDistance:'25%'},point(810),sc,false,0).concealed,false);
});
const peerScreen={name:'DP-1',width:2560,height:1440,scale:2};
const edgeEntry=(sc,patch={})=>({bar:{...edgeBar,id:'clock',key:sc.name+'/clock',syncMonitors:true,...patch},screen:sc,hovered:false,fading:false});
const edgeEntries=[edgeEntry(screen),edgeEntry(peerScreen)];
const hiddenGroup=()=>Object.fromEntries(edgeEntries.map(e=>[e.bar.key,{concealed:true}]));
const groupStep=(s,p,t,entries=edgeEntries)=>L.edgeStates(s,entries,p,t);
const allConcealed=(s,value)=>Object.values(s).forEach(v=>assert.equal(v.concealed,value));
test('shared activation uses the source dimensions, scale and delay',()=>{
 let s=hiddenGroup(), p={valid:true,monitor:'DP-1',x:10,y:1439};
 s=groupStep(s,p,0);allConcealed(s,true);
 s=groupStep(s,p,499);allConcealed(s,true);
 s=groupStep(s,p,500);allConcealed(s,false);
 s=groupStep(s,{...p,y:1230},600);allConcealed(s,false);
 s=groupStep(s,{...p,y:1229},700);
 s=groupStep(s,{...p,y:1229},1699);allConcealed(s,false);
 s=groupStep(s,{...p,y:1229},1700);allConcealed(s,true);
});
test('shared dwell cancels on departure and restarts on another screen',()=>{
 let s=groupStep(hiddenGroup(),point(1079),0);
 s=groupStep(s,point(1000),400);
 s=groupStep(s,point(1079),500);allConcealed(s,true);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:1439},900);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:1439},1399);allConcealed(s,true);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:1439},1400);allConcealed(s,false);
});
test('shared hover/menu, cross-screen traversal and fade reversal',()=>{
 let s=groupStep({},point(740),0);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:1230},1000);allConcealed(s,false);
 const held=edgeEntries.map((e,i)=>({...e,hovered:i===0}));
 s=groupStep(s,point(100),2000,held);
 s=groupStep(s,point(100),4000,held);allConcealed(s,false);
 s=groupStep(s,point(100),4100);s=groupStep(s,point(100),5100);allConcealed(s,true);
 s=groupStep(s,point(740),5200,edgeEntries.map(e=>({...e,fading:true})));allConcealed(s,false);
 s=groupStep(s,point(100),5300);s=groupStep(s,point(100),6300);allConcealed(s,true);
 s=groupStep(s,point(740),6400);allConcealed(s,true);
});
test('opt-out is independent in both directions and opted-out IDs stay hidden',()=>{
 for(const monitor of ['DP-1','DP-2']) {
  const entries=edgeEntries.map((e,i)=>({...e,bar:{...e.bar,syncMonitors:i===0,activationDelay:0,hideDelay:0}}));
  const extra=edgeEntry(screen,{id:'other',key:'DP-2/other',syncMonitors:false,activationEdge:'t',activationDelay:0,hideDelay:0});
  let s=groupStep({},point(500),0,[...entries,extra]);
  s=groupStep(s,{valid:true,monitor,x:10,y:monitor==='DP-1'?1439:1079},1,[...entries,extra]);
  assert.equal(s[monitor+'/clock'].concealed,false);
  assert.equal(s[(monitor==='DP-1'?'DP-2':'DP-1')+'/clock'].concealed,true);
  assert.equal(s['DP-2/other'].concealed,true);
 }
});
test('primary-only and different-edge bars synchronize with another monitor',()=>{
 const entries=[...edgeEntries,edgeEntry(peerScreen,{id:'primary-only',key:'DP-1/primary-only',activationEdge:'t',keepVisibleDistance:'50px'})];
 let s=groupStep({},point(500),0,entries);
 s=groupStep(s,point(500),1000,entries);allConcealed(s,true);
 s=groupStep(s,point(1079),1100,entries);
 s=groupStep(s,point(1079),1600,entries);allConcealed(s,false);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:20},1700,entries);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:20},3000,entries);allConcealed(s,false);
 s=groupStep(s,point(500),3100,entries);
 s=groupStep(s,point(500),4100,entries);allConcealed(s,true);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:1},4200,entries);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:1},4700,entries);allConcealed(s,false);
});
test('reload opt-out, removal, rejoin and failure leave no stale shared state',()=>{
 const entries=edgeEntries.map(e=>({...e,bar:{...e.bar,activationDelay:0,hideDelay:0}}));
 let s=groupStep(hiddenGroup(),point(1079),0,entries);allConcealed(s,false);
 const independent=entries.map(e=>({...e,bar:{...e.bar,syncMonitors:false}}));
 s=groupStep(s,point(1079),1,independent);assert.equal(s['DP-1/clock'].concealed,true);
 s=groupStep(s,point(1079),2,entries);allConcealed(s,false);
 s=groupStep(s,point(1079),3,[entries[1]]);assert.deepEqual(Object.keys(s),['DP-1/clock']);allConcealed(s,true);
 s=groupStep(s,{valid:false},4,entries);allConcealed(s,false);
 s=groupStep(s,point(500),5,entries);allConcealed(s,true);
 s=groupStep(s,point(500),6,entries.map(e=>({...e,bar:{...e.bar,autoHide:false}})));allConcealed(s,false);
 assert.equal(Object.keys(groupStep(s,point(500),7,[])).length,0);
});
test('per-monitor edges and delays follow the input source',()=>{
 const entries=[edgeEntries[0],edgeEntry(peerScreen,{activationEdge:'t',activationDelay:100,hideDelay:200})];
 let s=groupStep(hiddenGroup(),{valid:true,monitor:'DP-1',x:10,y:1},0,entries);
 s=groupStep(s,{valid:true,monitor:'DP-1',x:10,y:1},100,entries);allConcealed(s,false);
 s=groupStep(s,point(100,'absent'),200,entries);
 s=groupStep(s,point(100,'absent'),400,entries);allConcealed(s,true);
});
test('50 percent centre overflow stays on screen and borrows inward',()=>{
 for(const scale of [1,2])for(const side of ['t','b','l','r'])for(const position of ['l','r']) {
  const h=['t','b'].includes(side),span=h?1920:1080,content=span*.4;
  const g=L.geometry({...base,side,position,alignment:'l',overflow:'centre',layout:{centre_width:'50%'}},1920,1080,h?content:28,h?28:content,scale);
  const start=h?g.x:g.y, size=h?g.width:g.height;
  near(size,content);near(start,position==='r'?span-content:0);assert.ok(start>=0 && start+size<=span);
 }
 const g=L.geometry({...base,position:'r',overflow:'centre',layout:{centre_width:'50%'}},1920,1080,1200,28,1);near(g.x,960);near(g.width,960);assert.equal(g.overflow,true);
});
test('workspace preview preserves per-monitor logical window layout',()=>{
 const a=L.previewRect({at:[3540,820],size:[960,540]},{x:3440,y:720},1920,1080,320,180);
 near(a.x,17);near(a.y,17);near(a.width,160);near(a.height,90);
 const b=L.previewRect({at:[-1920,0],size:[1920,1080]},{x:-1920,y:0},1920,1080,320,180);near(b.x,0);near(b.y,0);near(b.width,320);
});
test('panel centred on triggering part and clamped to screen on all sides',()=>{
 const a={x:700,y:500,width:20,height:20},b={x:600,y:900,width:500,height:44};
 for(const side of ['t','b']){const p=L.panelAnchor(a,b,side,1920,1080,440,320);near(p.x,490);near(p.y,side==='b'?892:952);}
 for(const side of ['l','r']){const p=L.panelAnchor(a,b,side,1920,1080,440,320);near(p.y,350);near(p.x,side==='l'?1108:592);}
 near(L.panelAnchor({...a,x:0},b,'b',1920,1080,440,320).x,12);
 near(L.panelAnchor({...a,x:1910},b,'b',1920,1080,440,320).x,1468);
});
for(const side of ['t','b','l','r'])for(const scale of [1,2])test('edge-matched gutter '+side+' @'+scale,()=>{
 const h=['t','b'].includes(side),span=h?1920:1080,offset=side==='t'||side==='l'?60:-60;
 for(const position of ['l','r']) {
  const b={...base,side,position,alignment:position,overflow:'centre',layout:{centre_width:'50%'},screenGutter:'edgeOffset',offsetX:h?0:offset,offsetY:h?offset:0};
  const g=L.geometry(b,1920,1080,h?span:44,h?44:span,scale);
  const start=h?g.x:g.y,size=h?g.width:g.height;
  near(position==='l'?start:span-start-size,60/scale);
  near(size,span/2-60/scale);assert.equal(g.overflow,true);
 }
});
test('gutter supports percentages, explicit pixels and remains independent of columns',()=>{
 for(const [gutter,expected] of [['10%',192],['60px',30],[0,0]]) {
  const g=L.geometry({...base,alignment:'l',screenGutter:gutter},1920,1080,200,44,2);
  near(g.x,expected);near(g.columnSize,640);near(g.width,200);
 }
 const g=L.geometry({...base,alignment:'l',offsetY:'-10%',screenGutter:'edgeOffset'},1920,1080,200,44,2);near(g.x,108);
 const huge=L.geometry({...base,screenGutter:'90%'},1920,1080,200,44,1);near(huge.width,0);assert.ok(huge.x>=0);
});
test('gutter pressure borrows centre before clipping',()=>{
 for(const position of ['l','r']) {
  const g=L.geometry({...base,position,overflow:'centre',layout:{centre_width:'50%'},screenGutter:'edgeOffset',offsetY:-60},1920,1080,450,44,1);
  near(g.width,450);near(g.x,position==='l'?60:1410);assert.equal(g.overflow,false);
 }
});
test('ProIcons semantic mapping and Material fallback',()=>{
 const icons=vm.createContext({});vm.runInContext(readFileSync(new URL('../qml/IconRegistry.js',import.meta.url),'utf8'),icons);
 for(const name of Object.keys(icons.proicons)){const path=icons.resolve(name,'proicons');assert.ok(readFileSync(new URL('../qml/'+path,import.meta.url),'utf8').includes('<svg'));assert.equal(icons.resolve(name,'material'),'');}
 assert.equal(icons.resolve('unmapped_symbol','proicons'),'');
});
test('shared hover partitions padding and every gap without dead zones',()=>{
 const a={id:'a',x:12,y:10,width:80,height:24},b={id:'b',x:104,y:10,width:50,height:24};
 for(const x of [0,12,50,92,97])assert.equal(L.nearestTarget([a,b],x,2).id,'a');
 for(const x of [99,104,130,154,166])assert.equal(L.nearestTarget([a,b],x,42).id,'b');
 assert.equal(L.nearestTarget([],0,0),null);
});
test('morph endpoints and interrupted transition remain finite and continuous',()=>{
 const m=vm.createContext({}),d=vm.createContext({});vm.runInContext(readFileSync(new URL('../qml/MorphLogic.js',import.meta.url),'utf8'),m);vm.runInContext(readFileSync(new URL('../qml/MorphData.js',import.meta.url),'utf8'),d);
 for(const [a,b] of m.groups){assert.ok(m.compatible(a,b));const first=d.shapes[a],last=d.shapes[b],mid=m.interpolate(first,last,.4),back=m.interpolate(mid,first,0);
  assert.equal(JSON.stringify(mid),JSON.stringify(back));
  for(const t of [0,.25,.5,1])for(const path of m.interpolate(first,last,t))for(const p of path.points)assert.ok(p.every(Number.isFinite));
  assert.equal(JSON.stringify(m.interpolate(first,last,0)[0].points),JSON.stringify(first[0].points));
  m.interpolate(first,last,1)[0].points.forEach((p,i)=>p.forEach((v,j)=>near(v,last[0].points[i][j])));
 }
});


const T=vm.createContext({});vm.runInContext(readFileSync(new URL('../qml/TrayIcons.js',import.meta.url),'utf8'),T);
test('tray override matching is case insensitive, deterministic and literal',()=>{
 assert.equal(T.override({'chrome_status_icon_*':'chrome'},'Chrome_Status_Icon_1',''),'chrome');
 assert.equal(T.override({'zapzap':'whatsapp'},'something','ZapZap'),'whatsapp');
 assert.equal(T.override({'first*':'one','first':'two'},'first',''),'one');
 assert.equal(T.override({'a.b':'exact'},'axb',''),'');
 assert.equal(T.override({},'unknown',''),'');
});



const M=vm.createContext({});vm.runInContext(readFileSync(new URL('../qml/Media.js',import.meta.url),'utf8'),M);
test('Spotify artist and playlist destinations are typed and safe',()=>{
 assert.equal(M.spotifyUri('https://open.spotify.com/artist/abc123?si=test','artist'),'spotify:artist:abc123');
 assert.equal(M.playlistDestination({'spotify:contextUrl':'spotify:playlist:abc123'}),'spotify:playlist:abc123');
 assert.equal(M.playlistDestination({'xesam:url':'spotify:track:abc123'}),'');
 assert.equal(M.artistDestination({},'A & B; $(echo x)'), '');
 for(const uri of ['javascript:alert(1)','https://evil.example/artist/abc','https://open.spotify.com.evil/artist/abc','spotify:track:abc'])assert.equal(M.spotifyUri(uri,'artist'),'');
});
test('media playback artist and track clicks stay independent',()=>{
 const row={actions:{scrollUp:'player.previous'},icon:{actions:{leftClick:'player.toggle'}},artist:{actions:{leftClick:'player.artist'}},track:{actions:{leftClick:'player.playlist'}}};
 for(const [part,action] of [['icon','player.toggle'],['artist','player.artist'],['track','player.playlist']]){
  assert.equal(L.choose({},row,part,'leftClick'),action);assert.equal(L.choose({},row,part,'scrollUp'),'player.previous');
 }
});
test('tray exclusions accept app prefixes without hiding unrelated apps',()=>{
 assert.equal(T.excluded(['spotify*','Mullvad VPN*','solaar'],'Mullvad VPN_status_icon_1',''),true);
 assert.equal(T.excluded(['spotify*'],'spotify-client',''),true);
 assert.equal(T.excluded(['solaar'],'indicator-solaar','Solaar'),true);
 assert.equal(T.excluded(['spotify*'],'TelegramDesktop',''),false);
 assert.equal(T.override({'ChatGPT':'chatgpt'},'chrome_status_icon_1','','ChatGPT'),'chatgpt');
 assert.equal(T.override({'ChatGPT':'chatgpt'},'chrome_status_icon_1','','Other Electron App'),'');
});

test('tray app focus identifies ChatGPT without claiming other Chromium apps',()=>{
 assert.equal(T.focusApp('chrome_status_icon_1','','ChatGPT'),'chatgpt');
 assert.equal(T.focusApp('chrome_status_icon_1','','Chrome'),'');
 assert.equal(T.focusApp('TelegramDesktop','',''),'telegram');
 assert.equal(T.focusApp('ZapZap','',''),'zapzap');
 assert.equal(T.focusApp('codex','',''),'codex');
});
test('previous uses a one-second restart boundary and native fallback',()=>{
 assert.equal(M.previousAction(0,true,true),'previous');
 assert.equal(M.previousAction(1,true,true),'previous');
 assert.equal(M.previousAction(1.01,true,true),'restart');
 assert.equal(M.previousAction(200,true,true),'restart');
 assert.equal(M.previousAction(200,false,true),'previous');
 assert.equal(M.previousAction(200,true,false),'previous');
});
test('preview cards share height and retain the island, card and travel gap',()=>{
 assert.equal(L.previewHeight({},1440),360);
 assert.equal(L.previewHeight({previewHeight:400},1440),400);
 assert.equal(L.previewHeight({},300),276);
 const source={x:100,y:500,width:200,height:44}, anchor={x:80,y:492};
 const held=(x,y,monitor='DP-1')=>L.panelHeld({valid:true,x,y,monitor},'DP-1',source,anchor,440,360,'b');
 assert.equal(held(150,520),true);
 assert.equal(held(150,496),true);
 assert.equal(held(150,300),true);
 assert.equal(held(150,100),false);
 assert.equal(held(150,520,'DP-2'),false);
});
test('lyrics follows timestamps, gaps, pause positions and seeks',()=>{
 const rows=[{time:2,text:'One'},{time:5,text:''},{time:7.25,text:'Two'}];
 assert.equal(M.lyricLine([],5),-1);
 assert.equal(M.lyricLine(rows,-1),-1);
 assert.equal(M.lyricLine(rows,NaN),-1);
 assert.equal(M.lyricLine(rows,0),-1);
 assert.equal(M.lyricLine(rows,2),0);
 assert.equal(M.lyricLine(rows,4.99),0);
 assert.equal(M.lyricLine(rows,5),1);
 assert.equal(M.lyricLine(rows,7.25),2);
 assert.equal(M.lyricLine(rows,300),2);
 assert.equal(M.lyricLine(rows,3),0);
});
test('Spotify lyrics IDs accept native URI, web URL and MPRIS object path only',()=>{
 const id='1234567890abcdefghijkl';
 for(const value of ['spotify:track:'+id,'https://open.spotify.com/track/'+id+'?si=fixture','https://open.spotify.com/intl-en/track/'+id,'/com/spotify/track/'+id,'QVariant(QDBusObjectPath, QDBusObjectPath("/com/spotify/track/'+id+'"))']) {
  assert.equal(M.spotifyTrackId({'xesam:url':value}),id);
  assert.equal(M.spotifyTrackId({'mpris:trackid':value}),id);
 }
 for(const value of ['https://evil.test/track/'+id,'spotify:album:'+id,'spotify:track:short','file:///tmp/'+id,'https://open.spotify.com.evil.test/track/'+id])assert.equal(M.spotifyTrackId({'xesam:url':value}),'');
});
console.log(`bartjik: ${count} layout and interaction assertions passed`);

test('workspace thumbnails share physical pixel edges at fractional monitor scales',()=>{
 for(const scale of [1,1.25,1.5,2]) {
  const rect=(x,w)=>L.previewRect({at:[x-1920,0],size:[w,1080]},{x:-1920,y:0},1920,1080,813,457,scale);
  const left=rect(0,733),right=rect(733,1187);
  near(left.x+left.width,right.x);
  near(right.x+right.width,Math.round(813*scale)/scale);
  for(const r of [left,right])for(const v of Object.values(r))near(v*scale,Math.round(v*scale));
  assert.ok(rect(0,.01).width>=1/scale);
 }
});
