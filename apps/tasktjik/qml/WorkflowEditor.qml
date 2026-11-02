import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "DueDate.js" as DueDate

ColumnLayout {
    id: root
    property var theme
    property var nodes: []
    property bool sequential:false
    property string selectedId: ''
    property string problem: ''
    property var selectedNode: null
    spacing: 8
    component StepButton: Button {
        id:control
        implicitHeight:36
        contentItem:Label {text:control.text;color:control.highlighted?root.theme.accent:root.theme.text;wrapMode:Text.Wrap;verticalAlignment:Text.AlignVCenter}
        background:Rectangle {radius:6;color:control.hovered || control.highlighted?root.theme.surface:'transparent';border.color:control.visualFocus?root.theme.accent:root.theme.border}
    }
    function flatten(list, depth, parent) {
        let rows=[];
        for(const n of list) {
            rows.push({node:n,depth:depth,parent:parent});
            rows=rows.concat(flatten(n.items || [],depth+1,n));
        }
        return rows;
    }
    function uuid() {return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g,c=>{const n=Math.floor(Math.random()*16);return (c==='x'?n:(n&3)|8).toString(16);});}
    function change(id, fn) {
        const copy=JSON.parse(JSON.stringify(nodes));
        function visit(list) {for(let i=0;i<list.length;i++){if(list[i].id===id){fn(list,i);return true;}if(visit(list[i].items || []))return true;}return false;}
        visit(copy);nodes=copy;
    }
    function flush() {
        if(!selectedNode)return true;
        const parsed=DueDate.parse(deadline.text,new Date());
        if(!stepTitle.text.trim()){problem='Give the subtask a title.';return false;}
        if(parsed.error){problem=parsed.error;return false;}
        const kind=typePicker.alternative?typePicker.currentValue:selectedNode.kind;
        change(selectedId,(list,i)=>{list[i].title=stepTitle.text.trim();list[i].kind=kind;list[i].due=deadline.visible?parsed.value:'';list[i].offset=relative.text;});
        problem='';return true;
    }
    function select(node) {
        if(!flush())return;
        selectedId=node.id;selectedNode=JSON.parse(JSON.stringify(node));
        stepTitle.text=node.title;deadline.text=node.due || '';relative.text=node.offset || 'one week before';
        typePicker.currentIndex=Math.max(0,typePicker.model.findIndex(t=>t.value===node.kind));
    }
    function add(kind,parentId) {
        if(!flush())return;
        const node={id:uuid(),title:kind==='flow'?'Choose an alternative':'New subtask',kind:kind,due:'',done:false,items:[]};
        if(kind==='flow')node.items=[{id:uuid(),title:'Alternative',kind:'task',due:'',done:false,items:[]}];
        if(parentId)change(parentId,(list,i)=>list[i].items.push(node));
        else nodes=nodes.concat([node]);
        selectedNode=null;select(node);stepTitle.forceActiveFocus();stepTitle.selectAll();
    }
    function reset(value, workflow) {sequential=!!workflow;selectedId='';selectedNode=null;problem='';nodes=JSON.parse(JSON.stringify(value || []));}
    RowLayout {
        Label {text:'Subtasks & flows';font.bold:true;color:root.theme.text;Layout.fillWidth:true}
        StepButton {objectName:'workflow-toggle';text:'Workflow';checkable:true;checked:root.sequential;highlighted:checked;onClicked:root.sequential=checked;Accessible.name:'Workflow: each primary step depends on the previous step'}
    }
    Label {Layout.fillWidth:true;text:root.sequential?'Primary steps wait for the preceding step. A chosen flow waits for its branches before continuing.':'Independent steps. Enable Workflow to link primary steps in order.';wrapMode:Text.Wrap;color:root.theme.muted;font.pixelSize:12}
    Repeater {
        model:root.flatten(root.nodes,0,null)
        StepButton {
            required property var modelData
            Layout.fillWidth:true;Layout.leftMargin:modelData.depth*18
            text:(modelData.node.kind==='flow'?'⑂ ':modelData.node.kind==='exit'?'EXIT · ':modelData.node.kind==='reminder'?'REMINDER · ':'')+modelData.node.title+(modelData.node.due?' · '+modelData.node.due:'')
            highlighted:root.selectedId===modelData.node.id
            onClicked:root.select(modelData.node)
        }
    }
    RowLayout {
        StepButton {text:'+ Subtask';onClicked:root.add('task','')}
        StepButton {text:'+ Flow';onClicked:root.add('flow','')}
    }
    ColumnLayout {
        visible:!!root.selectedNode;Layout.fillWidth:true;spacing:8
        TextField {id:stepTitle;objectName:'subtask-title';Layout.fillWidth:true;placeholderText:'Subtask title';Accessible.name:'Subtask title'}
        ComboBox {
            id:typePicker;Layout.fillWidth:true
            readonly property bool alternative:root.flatten(root.nodes,0,null).some(r=>r.node.id===root.selectedId && r.parent && r.parent.kind==='flow')
            visible:alternative
            textRole:'label';valueRole:'value'
            model:[{label:'Alternative',value:'task'},{label:'Exit — cancel all subsequent steps',value:'exit'},{label:'Reminder — keep alternatives open',value:'reminder'}]
        }
        TextField {
            id:deadline;Layout.fillWidth:true
            visible:!typePicker.alternative
            placeholderText:'Due date (e.g. 20 November)';Accessible.name:'Step or flow deadline'
        }
        TextField {id:relative;Layout.fillWidth:true;visible:typePicker.alternative && typePicker.currentValue==='reminder';placeholderText:'one week before';Accessible.name:'Reminder relative to flow deadline'}
        RowLayout {
            StepButton {text:'Apply step';onClicked:root.flush()}
            StepButton {
                text:root.selectedNode && root.selectedNode.kind==='flow'?'+ Alternative':'+ Branch subtask'
                visible:root.selectedNode && (root.selectedNode.kind==='flow' || (typePicker.alternative && typePicker.currentValue==='task'))
                onClicked:root.add('task',root.selectedId)
            }
            StepButton {text:'Remove';onClicked:{root.change(root.selectedId,(list,i)=>list.splice(i,1));root.selectedNode=null;root.selectedId='';}}
        }
    }
    Label {visible:!!root.problem;text:root.problem;color:'#eb6f92';wrapMode:Text.Wrap;Layout.fillWidth:true}
}
