import cytoscape from 'cytoscape';
import Sigma from 'sigma';
import Graph from 'graphology';

const frame=()=>new Promise(resolve=>requestAnimationFrame(resolve));
const percentile=(xs,p)=>[...xs].sort((a,b)=>a-b)[Math.min(xs.length-1,Math.floor(xs.length*p))];
function dataset(n){
 const nodes=Array.from({length:n},(_,i)=>({id:String(i),label:`Документ ${i}: условие и источник`,x:Math.cos(i*2.399963)*Math.sqrt(i+1)*12,y:Math.sin(i*2.399963)*Math.sqrt(i+1)*12}));
 const edges=[];for(let i=1;i<n;i++){edges.push({id:`e${edges.length}`,source:String(i),target:String(Math.floor((i-1)/2))});if(i>8)edges.push({id:`e${edges.length}`,source:String(i),target:String(i-7)});}
 return {nodes,edges};
}
window.probe=async function(renderer,n){
 const data=dataset(n),container=document.getElementById('map');container.replaceChildren();let instance;
 const started=performance.now();
 if(renderer==='cytoscape'){
  instance=cytoscape({container,elements:[...data.nodes.map(({x,y,...data})=>({data,position:{x,y}})),...data.edges.map(data=>({data}))],layout:{name:'preset'},style:[{selector:'node',style:{label:'data(label)','font-size':10,'background-color':'#be486b',width:14,height:14}},{selector:'edge',style:{width:1,'line-color':'#b6b0ab','curve-style':'haystack'}},{selector:'.picked',style:{'background-color':'#183952','border-width':3,'border-color':'#df9e3c'}}],pixelRatio:1});
 }else{
  const graph=new Graph({multi:true});for(const node of data.nodes)graph.addNode(node.id,{...node,size:7,color:'#be486b'});
  for(const edge of data.edges)graph.addEdgeWithKey(edge.id,edge.source,edge.target,{size:1,color:'#b6b0ab'});
  instance=new Sigma(graph,container,{renderLabels:true,labelSize:10,enableEdgeEvents:true});
 }
 await frame();await frame();const firstFrame=performance.now()-started;
 const frames=[];for(let i=0;i<90;i++){
  const start=performance.now();
  if(renderer==='cytoscape'){instance.panBy({x:i%2?2:-2,y:0});instance.zoom(instance.zoom()*(i%2?1.002:1/1.002));}
  else{const camera=instance.getCamera(),state=camera.getState();camera.setState({x:state.x+(i%2?.001:-.001),ratio:state.ratio*(i%2?1.002:1/1.002)});}
  await frame();frames.push(performance.now()-start);
 }
 const selectStart=performance.now();
 if(renderer==='cytoscape'){instance.$id('10').addClass('picked');instance.$id('10').lock();instance.$id('10').unlock();instance.layout({name:'preset'}).stop();}
 else{instance.getGraph().setNodeAttribute('10','color','#183952');instance.refresh();}
 await frame();const select=performance.now()-selectStart;
 const disposeStart=performance.now();if(renderer==='cytoscape')instance.destroy();else instance.kill();
 const result={renderer,n,edges:data.edges.length,first_frame_ms:firstFrame,frame_p50_ms:percentile(frames,.5),frame_p95_ms:percentile(frames,.95),select_ms:select,dispose_ms:performance.now()-disposeStart,remaining_canvas:container.querySelectorAll('canvas').length};
 document.getElementById('report').textContent=JSON.stringify(result,null,2);return result;
};
