function pips(percent) {
    if (percent < 5) return 9;
    return Math.max(0, Math.min(9, Math.ceil((percent - 1) / 10)));
}
function tone(count, percent) { return percent < 5 ? 'red' : count >= 6 ? 'green' : count >= 3 ? 'yellow' : 'red'; }

// 24px viewbox, matching the surrounding rounded 1.5px outline icons.
var grid = Array.from({length:9}, function(_, i) {
    const x=5+(i%3)*7, y=5+Math.floor(i/3)*7;
    return {points:Array.from({length:48}, function(_, j) {
        const angle=2*Math.PI*j/47;
        return [x+1.65*Math.cos(angle),y+1.65*Math.sin(angle)];
    }),closed:true,fill:1,stroke:0};
});
var bolt = [[13.5,3],[6,13],[11,13],[10.5,21],[18,10],[13,10],[13.5,3]];
function sample(vertices, count) {
    const lengths=vertices.slice(1).map((p,i)=>Math.hypot(p[0]-vertices[i][0],p[1]-vertices[i][1]));
    const total=lengths.reduce((a,b)=>a+b,0);
    return Array.from({length:count},function(_,i) {
        let distance=total*i/(count-1), edge=0;
        while(edge<lengths.length-1 && distance>lengths[edge])distance-=lengths[edge++];
        const t=distance/lengths[edge],a=vertices[edge],b=vertices[edge+1];
        return [a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t];
    });
}
var flash = grid.map(function(path,i) {
    return {points:i===4?sample(bolt,48):path.points.map(()=>[12,12]),closed:true,fill:0,stroke:i===4?1:0,opacity:i===4?1:0};
});
