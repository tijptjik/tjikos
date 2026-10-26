var groups=[['notifications','notifications_off'],['coffee','hotel'],['nightlight','light_mode'],['desktop_windows','sports_esports'],['volume_up','headphones']];
function compatible(a,b){return a!==b && groups.some(g=>g.includes(a)&&g.includes(b));}
function interpolate(a,b,t){
    const result=[];
    for(let i=0;i<Math.max(a.length,b.length);i++){
        const x=a[i],y=b[i],reference=x || y;
        const centre=reference.points.reduce((p,q)=>[p[0]+q[0]/reference.points.length,p[1]+q[1]/reference.points.length],[0,0]);
        const points=reference.points.map((_,j)=>{const p=x?.points[j] || centre,q=y?.points[j] || centre;return [p[0]+(q[0]-p[0])*t,p[1]+(q[1]-p[1])*t];});
        const lerp=(key,defaultValue)=>((x?.[key] ?? defaultValue)*(1-t)+(y?.[key] ?? defaultValue)*t);
        result.push({points:points,closed:t<.5?x?.closed:y?.closed,fill:lerp('fill',0),stroke:lerp('stroke',0),opacity:((x?.opacity ?? (x?1:0))*(1-t)+(y?.opacity ?? (y?1:0))*t)});
    }
    return result;
}
