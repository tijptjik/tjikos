function size(m) {
    const s=m.settings || m, match=String(s.mode || '').match(/^(\d+)x(\d+)@/);
    if(!match)return {width:0,height:0};
    const rotated=Number(s.transform || 0)%2===1, scale=Number(s.scale || 1);
    return {width:Number(match[rotated?2:1])/scale,height:Number(match[rotated?1:2])/scale};
}
function bounds(monitors) {
    const rects=monitors.filter(m=>!mirrorSource(m,monitors)).map(m=>{const s=m.settings || m,z=size(m);return {x:Number(s.x || 0),y:Number(s.y || 0),width:z.width,height:z.height};}).filter(r=>r.width>0 && r.height>0);
    if(!rects.length)return {x:0,y:0,width:1,height:1};
    const x=Math.min(...rects.map(r=>r.x)),y=Math.min(...rects.map(r=>r.y));
    return {x:x,y:y,width:Math.max(...rects.map(r=>r.x+r.width))-x,height:Math.max(...rects.map(r=>r.y+r.height))-y};
}
function fit(box,width,height) {
    const scale=Math.min((width-28)/Math.max(1,box.width),(height-28)/Math.max(1,box.height));
    return {scale:scale,x:(width-box.width*scale)/2-box.x*scale,y:(height-box.height*scale)/2-box.y*scale};
}
function snap(m,monitors,x,y,threshold) {
    const z=size(m);let bestX=threshold,bestY=threshold,dx=0,dy=0;
    for(const peer of monitors){
        if(peer.name===m.name || mirrorSource(peer,monitors))continue;
        const p=peer.settings,w=size(peer);
        for(const edge of [p.x,p.x+w.width])for(const moving of [x,x+z.width]){const d=edge-moving;if(Math.abs(d)<bestX){bestX=Math.abs(d);dx=d;}}
        for(const edge of [p.y,p.y+w.height])for(const moving of [y,y+z.height]){const d=edge-moving;if(Math.abs(d)<bestY){bestY=Math.abs(d);dy=d;}}
    }
    return {x:Math.max(-32768,Math.min(32768,Math.round(x+dx))),y:Math.max(-32768,Math.min(32768,Math.round(y+dy)))};
}
function changes(before,after) {
    const fields=['enabled','mode','x','y','scale','transform','vrr','cm','mirror'];
    return after.map(m=>{
        const old=before.find(b=>b.name===m.name), settings={};
        if(old && m.connected)for(const key of fields)if(old.settings[key]!==m.settings[key])settings[key]=m.settings[key];
        return {name:m.name,settings:settings};
    }).filter(e=>Object.keys(e.settings).length);
}
function mirrorSource(m,monitors) {
    if(!m.connected || !m.settings?.enabled || !m.settings.mirror)return null;
    return monitors.find(p=>p.name===m.settings.mirror && p.connected && p.settings.enabled && !p.settings.mirror) || null;
}
function mirrors(m,monitors) {
    return monitors.filter(p=>mirrorSource(p,monitors)?.name===m.name);
}
function scales(m) {
    const dimensions=String(m.settings.mode).match(/^(\d+)x(\d+)@/);
    const choices=[.5,.75,1,1.25,1.5,1.75,2,2.5,3,4,Number(m.settings.scale)].sort((a,b)=>a-b);
    return [...new Set(choices)].filter(s=>dimensions && [Number(dimensions[1]),Number(dimensions[2])].every(d=>Math.abs(d/s-Math.round(d/s))<.01));
}
