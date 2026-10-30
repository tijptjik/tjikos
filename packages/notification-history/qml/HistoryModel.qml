import QtQuick
import "Logic.js" as Logic

QtObject {
    id:model
    property var items:[]
    property string query:''
    property bool grouping:true
    property string selected:''
    property var expanded:({})
    property var searchExpanded:({})
    readonly property bool filtering:query.trim().length>0
    readonly property var filtered:Logic.filterHistory(items,query)
    readonly property var groups:Logic.historyGroups(filtered,grouping)
    readonly property var rows:{
        const rows=[];
        for(const group of groups){
            rows.push({kind:'group',key:group.key,app:group.app,label:group.label,count:group.notices.length});
            if(isExpanded(group.key))for(const notice of group.notices)rows.push({kind:'notice',key:notice.historyKey,notice:notice});
        }
        return rows;
    }
    onQueryChanged:searchExpanded=({})
    onFilteredChanged:if(!filtered.some(n=>n.historyKey===selected))selected=''
    function isExpanded(key){return filtering?searchExpanded[key]!==false:!!expanded[key];}
    function toggle(key){
        if(filtering)searchExpanded=Object.assign({},searchExpanded,{[key]:!isExpanded(key)});
        else expanded=Object.assign({},expanded,{[key]:!expanded[key]});
    }
    function select(key){
        selected=key;
        const group=groups.find(g=>g.notices.some(n=>n.historyKey===key));
        if(group && !isExpanded(group.key))toggle(group.key);
    }
    function navigate(direction){
        if(!filtered.length)return;
        let index=filtered.findIndex(n=>n.historyKey===selected);
        index=index<0?0:(index+(direction==='previous'?1:-1)+filtered.length)%filtered.length;
        select(filtered[index].historyKey);
    }
    function reset(){query='';selected='';expanded=({});}
}
