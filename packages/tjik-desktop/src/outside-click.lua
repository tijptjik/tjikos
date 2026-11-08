local pid=__PID__
_G.runtjik_gtk_watchers=_G.runtjik_gtk_watchers or {}
local watchers=_G.runtjik_gtk_watchers
if watchers[pid] then watchers[pid].pinned=__PINNED__;return end
local state={pinned=__PINNED__};watchers[pid]=state
local bindings,closed={},nil
local function cleanup()
    for _,binding in ipairs(bindings) do binding:unbind() end
    if closed then closed:remove();closed=nil end
    watchers[pid]=nil
end
local function pressed()
    local target=nil
    for _,layer in ipairs(hl.get_layers()) do
        if layer.pid==pid and layer.namespace=='runtjik-gtk' then target=layer;break end
    end
    if not target then cleanup();return end
    local point=hl.get_cursor_pos()
    local geometry={x=target.x,y=target.y,width=target.w,height=target.h}
    if point and geometry then
        if point.x<geometry.x or point.x>=geometry.x+geometry.width or point.y<geometry.y or point.y>=geometry.y+geometry.height then
            hl.dispatch(hl.dsp.exec_cmd(__COMMAND__))
        end
    end
end
for button=272,279 do table.insert(bindings,hl.bind('mouse:'..button,pressed,{non_consuming=true,ignore_mods=true,transparent=true,submap_universal=true})) end
closed=hl.on('layer.closed',function(layer) if layer.pid==pid and layer.namespace=='runtjik-gtk' then cleanup() end end)
hl.layer_rule({name='runtjik-gtk-plain',match={namespace='^runtjik-gtk$'},blur=false,no_anim=true,order=100})
