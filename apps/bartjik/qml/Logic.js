// Pure layout/interaction functions, also executed by the Bun regression suite.
function pixels(value, extent, scale) {
    if (typeof value === 'string' && value.endsWith('%')) return parseFloat(value) * extent / 100;
    return parseFloat(value || 0) / (scale || 1);
}
function columns(layout, span, scale) {
    const c = layout.centre_width === undefined ? span / 3 : pixels(layout.centre_width, span, scale);
    if (!isFinite(c) || c < 0 || c > span) throw Error('centre_width exceeds screen span');
    const l = (span - c) / 2;
    return {l: {start: 0, size: l}, c: {start: l, size: c}, r: {start: l + c, size: l}};
}
function geometry(bar, sw, sh, contentWidth, contentHeight, scale) {
    const horizontal = bar.side === 't' || bar.side === 'b';
    const column = columns(bar.layout || {}, horizontal ? sw : sh, scale)[bar.position];
    const span=horizontal?sw:sh;
    const pad = bar.padding || 0;
    let available = Math.max(0, column.size - 2 * pad);
    let origin=column.start+pad;
    const content=horizontal?contentWidth:contentHeight;
    const gutter = bar.screenGutter==='edgeOffset'
        ? Math.abs(pixels(horizontal?bar.offsetY:bar.offsetX,horizontal?sh:sw,scale))
        : pixels(bar.screenGutter,span,scale);
    const shift = pixels(horizontal?bar.offsetX:bar.offsetY,span,scale);
    const safeGutter=Math.min(gutter,span/2);
    const columnAvailable=Math.max(0,Math.min(span-safeGutter,origin+available+shift)-Math.max(safeGutter,origin+shift));
    // Gutter pressure can trigger inward overflow even when the raw column would fit.
    const expanded=bar.overflow==='centre' && bar.position!=='c' && content>columnAvailable;
    if(expanded) {
        available=Math.max(0,span/2-2*pad);
        if(bar.position==='r')origin=span/2+pad;
    }
    // Intersect the selected (possibly inward-expanded) section with screen gutters.
    // This leaves the shared column boundaries unchanged and clips only when necessary.
    const low=Math.max(safeGutter,Math.min(span-safeGutter,origin+shift));
    const high=Math.max(low,Math.min(span-safeGutter,origin+available+shift));
    available=high-low;
    const extent = Math.min(content, available);
    const align=expanded ? (bar.position==='r'?'r':'l') : bar.alignment;
    const start=low+(align==='l'?0:align==='r'?available-extent:(available-extent)/2);
    const depth = bar.stackOffset || 0;
    let x = horizontal ? start : bar.side === 'l' ? depth : sw - contentWidth - depth;
    let y = horizontal ? bar.side === 't' ? depth : sh - contentHeight - depth : start;
    if(horizontal)y += pixels(bar.offsetY,sh,scale);
    else x += pixels(bar.offsetX,sw,scale);
    return {x:x, y:y, width:horizontal ? extent : contentWidth, height:horizontal ? contentHeight : extent,
        columnStart:column.start, columnSize:column.size, overflow:(horizontal ? contentWidth : contentHeight) > available};
}
// JSON snapshots are immutable. Reuse unchanged branches so a clock/provider
// update does not invalidate every bar, module, theme and item binding.
function reconcile(previous, next) {
    if(previous===next)return previous;
    if(!previous || !next || typeof previous!=='object' || typeof next!=='object'
        || Array.isArray(previous)!==Array.isArray(next))return next;
    const keys=Object.keys(next);
    let equal=keys.length===Object.keys(previous).length;
    for(const key of keys) {
        const value=reconcile(previous[key],next[key]);
        if(!Object.prototype.hasOwnProperty.call(previous,key) || value!==previous[key])equal=false;
        next[key]=value;
    }
    return equal?previous:next;
}
// Styles are also immutable: empty/equivalent overrides need no copies.
function merge(base, patch) {
    let out=base;
    for(const k of Object.keys(patch || {})) {
        const value=patch[k] && typeof patch[k]==='object' && !Array.isArray(patch[k])
            && base[k] && typeof base[k]==='object' && !Array.isArray(base[k])
            ? merge(base[k],patch[k]) : patch[k];
        if(value===base[k] && Object.prototype.hasOwnProperty.call(base,k))continue;
        if(out===base)out=Object.assign({},base);
        out[k]=value;
    }
    return out;
}
function choose(module, item, part, event) {
    if (event === 'click') event = 'leftClick';
    let selected = null;
    function apply(actions) {
        if (event === 'leftClick' && actions.click !== undefined) selected = actions.click;
        if (actions[event] !== undefined) selected = actions[event];
    }
    apply(item.actions || {}); apply((item[part] || {}).actions || {});
    [module, (module.items || {})[item.id] || {}].forEach(p => {apply(p.actions || {}); apply((p[part] || {}).actions || {});});
    return selected;
}
function partStyle(theme, module, item, part, hovered) {
    let style = merge(theme, module.style || {});
    style = merge(style, ((module.items || {})[item.id] || {}).style || {});
    Object.keys(item.state || {}).forEach(k => {if(item.state[k]) style = merge(style,(style.states || {})[k] || {});});
    style = merge(style,(item[part] || {}).style || {});
    style = merge(style,(module[part] || {}).style || {});
    style = merge(style,((((module.items || {})[item.id] || {})[part] || {}).style || {}));
    if (hovered) style = merge(style,(style.states || {}).hover || {});
    return style;
}
function sync(model, keys) {
    for (let i=model.count-1;i>=0;--i) if(keys.indexOf(model.get(i).key)<0) model.remove(i);
    for (let i=0;i<keys.length;++i) {
        if(i<model.count && model.get(i).key===keys[i]) continue;
        let old=-1;
        for(let j=i;j<model.count;++j) if(model.get(j).key===keys[i]) {old=j;break;}
        if(old>=0) model.move(old,i,1); else model.insert(i,{key:keys[i]});
    }
}
// Single-click waits only when a double action exists; a double cancels that pending click.
function click(state, event, hasDouble) {
    if(event==='doubleClick') {state.pending=false; return {cancel:true,fire:'doubleClick'};}
    if(event==='leftClick' && hasDouble) {state.pending=true;return {delay:true};}
    return {fire:event};
}
function transition(state, inside) {
    if(state.inside===inside)return null;
    state.inside=inside;return inside?'hoverEnter':'hoverLeave';
}

// Share one visibility state across participating bars on every monitor.
// Read pointer coordinates and timing from the source monitor, never a peer's
// geometry. Only actual pointer/hover input holds the group open.
function edgeStates(previous, entries, pointer, now) {
    const groups = {}, next = {};
    entries.forEach(entry=>{
        const b=entry.bar;
        const key=JSON.stringify(b.autoHide && b.syncMonitors!==false ? ['shared'] : ['local',b.key]);
        (groups[key] || (groups[key]=[])).push(entry);
    });
    Object.values(groups).forEach(group=>{
        let state=group.map(e=>previous[e.bar.key]).find(s=>s) || {};
        const last=group.find(e=>e.bar.key===state.source);
        const inputs=e=>edgeInput(e.bar,pointer,e.screen,e.hovered);
        const source=(last && inputs(last).atEdge ? last : group.find(e=>inputs(e).atEdge))
            || (last && inputs(last).held ? last : group.find(e=>inputs(e).held))
            || last || group.find(e=>e.screen.name===pointer?.monitor) || group[0];
        // Edge dwell must be continuous on one monitor, even across hotplug.
        if(state.source!==source.bar.key)state=Object.assign({},state,{enter:null});
        const result=edgeState(state,source.bar,pointer,source.screen,
            group.some(e=>e.hovered),now,group.some(e=>e.fading));
        result.source=source.bar.key;
        group.forEach(e=>{next[e.bar.key]=result;});
    });
    return next;
}

function edgeInput(bar, pointer, screen, hovered) {
    if(!pointer?.valid)return {atEdge:false,held:!!hovered};
    const edge=bar.activationEdge==='side' ? bar.side : bar.activationEdge;
    const same=pointer.monitor===screen.name;
    const extent=(edge==='t'||edge==='b')?screen.height:screen.width;
    const distance={t:pointer.y,b:screen.height-pointer.y,l:pointer.x,r:screen.width-pointer.x}[edge];
    const atEdge=same && distance>=0 && distance<=Math.max(1,pixels(bar.revealSize,extent,screen.scale));
    const held=hovered || (same && distance>=0 && distance<=Math.max(pixels(bar.keepVisibleDistance,extent,screen.scale),pixels(bar.revealSize,extent,screen.scale)));
    return {atEdge:atEdge,held:held};
}

// Keyboard reveal is held until a second invocation or actual edge contact.
function toggleReveal(state) {
    const held=!(state && state.keyboardHeld);
    return {concealed:!held,keyboardHeld:held,keyboardClosed:!held,enter:null,leave:null};
}

// Pure edge hysteresis. State stores deadlines, never pointer event counts.
function edgeState(state, bar, pointer, screen, hovered, now, fading) {
    const s = Object.assign({concealed:false,enter:null,leave:null},state);
    if(s.keyboardClosed) {
        if(!edgeInput(bar,pointer,screen,hovered).atEdge)return s;
        s.keyboardClosed=false;
    }
    if(s.keyboardHeld) {
        if(!edgeInput(bar,pointer,screen,hovered).atEdge)
            return Object.assign(s,{concealed:false,enter:null,leave:null});
        s.keyboardHeld=false;
    }
    if (!bar.autoHide || !pointer?.valid) return {concealed:false,enter:null,leave:null};
    const input=edgeInput(bar,pointer,screen,hovered), atEdge=input.atEdge, held=input.held;
    if (s.concealed && fading && held) return {concealed:false,enter:null,leave:null};
    if (s.concealed) {
        s.leave=null;
        if (!atEdge) s.enter=null;
        else {
            if(s.enter===null)s.enter=now;
            if(now-s.enter>=bar.activationDelay){s.concealed=false;s.enter=null;}
        }
    } else {
        s.enter=null;
        if(held)s.leave=null;
        else {
            if(s.leave===null)s.leave=now;
            if(now-s.leave>=bar.hideDelay){s.concealed=true;s.leave=null;}
        }
    }
    return s;
}

function previewRect(client,monitor,screenWidth,screenHeight,width,height,scale=1) {
    const at=client.at || [monitor.x,monitor.y], size=client.size || [screenWidth,screenHeight];
    // Round shared edges, rather than widths, so adjacent windows stay adjacent.
    const snap=n=>Math.round(n*scale)/scale;
    const x=(at[0]-monitor.x)*width/screenWidth,y=(at[1]-monitor.y)*height/screenHeight;
    return {x:snap(x),y:snap(y),
        width:Math.max(1/scale,snap(x+size[0]*width/screenWidth)-snap(x)),
        height:Math.max(1/scale,snap(y+size[1]*height/screenHeight)-snap(y))};
}

function detailHover(module, row) {
    if(row.idle)return false;
    if(['audio','network'].includes(module.provider))
        return module.panel?.hover!==false && (module.panel?.hover===true || choose(module,row,'item','hoverEnter')===module.provider+'.panel');
    return ['crypto','datetime','mullvad','tailscale','locale','codex','openrouter','playerctl'].includes(module.provider) && module.panel?.hover===true;
}

// Centre a popup on its triggering part, clamping only to the target screen.
function panelAnchor(anchor, bar, side, sw, sh, width, height) {
    const clamp=(n,extent,size)=>Math.max(12,Math.min(extent-size-12,n-size/2));
    return side==='t'||side==='b' ? {x:clamp(anchor.x+anchor.width/2,sw,width),y:side==='b'?bar.y-8:bar.y+bar.height+8}
        : {x:side==='l'?bar.x+bar.width+8:bar.x-8,y:clamp(anchor.y+anchor.height/2,sh,height)};
}

// Partition gaps and outer padding at the midpoint between adjacent item rectangles.
function nearestTarget(targets,x,y) {
    let best=null,distance=Infinity;
    targets.forEach(t=>{
        const dx=Math.max(t.x-x,0,x-t.x-t.width),dy=Math.max(t.y-y,0,y-t.y-t.height);
        const d=dx*dx+dy*dy;
        if(d<distance){best=t;distance=d;}
    });
    return best;
}

// Give the pointer time to cross an island gap without losing the open shell.
function panelCloseDelay(options) { return Math.max(250, options?.closeDelay ?? 160); }

// One card height across market, workspace and media previews.
function previewHeight(style, screenHeight) {
    return Math.min(style?.previewHeight ?? 360, Math.max(120,(screenHeight || 1080)-24));
}
// Keep the source island, popup, and the small travel corridor between them live.
function panelHeld(pointer, screen, source, anchor, width, height, side) {
    if(!pointer?.valid || pointer.monitor!==screen || !source)return false;
    const panel={x:side==='r'?anchor.x-width:anchor.x,
        y:side==='b'?anchor.y-height:anchor.y,width:width,height:height};
    const inside=r=>pointer.x>=r.x && pointer.x<=r.x+r.width && pointer.y>=r.y && pointer.y<=r.y+r.height;
    if(inside(source) || inside(panel))return true;
    const horizontal=side==='t'||side==='b';
    const gap=horizontal?{x:Math.min(source.x,panel.x),width:Math.max(source.x+source.width,panel.x+panel.width)-Math.min(source.x,panel.x),
        y:side==='b'?panel.y+panel.height:source.y+source.height,height:side==='b'?source.y-panel.y-panel.height:panel.y-source.y-source.height}
        :{y:Math.min(source.y,panel.y),height:Math.max(source.y+source.height,panel.y+panel.height)-Math.min(source.y,panel.y),
        x:side==='r'?panel.x+panel.width:source.x+source.width,width:side==='r'?source.x-panel.x-panel.width:panel.x-source.x-source.width};
    return gap.width>=0 && gap.height>=0 && inside(gap);
}
