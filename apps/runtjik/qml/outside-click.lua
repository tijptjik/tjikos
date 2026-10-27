-- Passive outside-click observer for the masked top-layer launcher.
return function(pid, x, y, width, height, pinned)
    local mapped=false
    for _, layer in ipairs(hl.get_layers()) do
        if layer.pid==pid and layer.namespace=='runtjik' then mapped=true;break end
    end
    if not mapped then return end
    _G.runtjik_click_watchers = _G.runtjik_click_watchers or {}
    local watchers = _G.runtjik_click_watchers
    if watchers[pid] then
        watchers[pid].pinned = pinned
        watchers[pid].bounds = {x=x,y=y,width=width,height=height}
        return
    end
    local state = {pinned=pinned,bounds={x=x,y=y,width=width,height=height}}
    local bindings, closed = {}, nil
    local function cleanup()
        for _, binding in ipairs(bindings) do binding:unbind() end
        if closed then closed:remove(); closed=nil end
        watchers[pid]=nil
    end
    watchers[pid]=state
    local function pressed()
        local layer=nil
        for _, candidate in ipairs(hl.get_layers()) do
            if candidate.pid==pid and candidate.namespace=='runtjik' then layer=candidate;break end
        end
        if not layer then cleanup();return end
        local point=hl.get_cursor_pos()
        local b=state.bounds
        local outside=false
        if point then
            local cx=math.max(b.x+22,math.min(point.x,b.x+b.width-22))
            local cy=math.max(b.y+22,math.min(point.y,b.y+b.height-22))
            outside=(point.x-cx)^2+(point.y-cy)^2>22^2
        end
        if outside then
            local action='outsideClick'
            hl.dispatch(hl.dsp.exec_cmd('quickshell ipc --pid '..pid..' call runtjik '..action))
        elseif state.pinned and point and point.x>=b.x+60 and point.x<=b.x+b.width-20
                and point.y>=b.y+20 and point.y<=b.y+66 then
            -- Reacquire typing focus only in the search field. Changing keyboard
            -- interactivity during a header press can cancel the pointer grab.
            hl.dispatch(hl.dsp.exec_cmd('quickshell ipc --pid '..pid..' call runtjik focusSearch'))
        end
    end
    for button=272,279 do
        table.insert(bindings,hl.bind('mouse:'..button,pressed,{
            non_consuming=true,ignore_mods=true,transparent=true,submap_universal=true,
        }))
    end
    closed=hl.on('layer.closed',function(layer)
        if layer.pid==pid and layer.namespace=='runtjik' then cleanup() end
    end)
end
