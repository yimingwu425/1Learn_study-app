import {useCallback,useEffect,useRef,useState} from 'react';
import {request} from './api';

export type StudyMemory={lastId?:string;questions:Record<string,{mode:string;hint:number;draft:string}>};
export const memoryKey='study-app-v2.learning';
const pendingKey=memoryKey+'.pending';
type Pending={questions:Record<string,number>;lastId:number};
function readMemory():StudyMemory {
 try {const m=JSON.parse(localStorage.getItem(memoryKey)||'{}');return {lastId:typeof m.lastId==='string'?m.lastId:undefined,questions:m.questions&&typeof m.questions==='object'&&!Array.isArray(m.questions)?m.questions:{}}} catch {return {questions:{}}}
}
function readPending():Pending {
 try {const p=JSON.parse(localStorage.getItem(pendingKey)||'{}');return {questions:Object.fromEntries(Object.keys(p.questions||{}).map(k=>[k,1])),lastId:p.lastId?1:0}} catch {return {questions:{},lastId:0}}
}

// Keep pending edits until the matching write has succeeded. A failed initial
// read must never turn an empty browser cache into a replacement server state.
export function useStudyMemory() {
 const [memory,renderMemory]=useState<StudyMemory>(readMemory);
 const current=useRef(memory),pending=useRef<Pending>(readPending()),version=useRef(1);
 const ready=useRef(false),inFlight=useRef(false),paused=useRef(false),mounted=useRef(false);
 const [revision,setRevision]=useState(0),[reload,setReload]=useState(0),[error,setError]=useState(''),[hydrated,setHydrated]=useState(false);
 const persist=useCallback(()=>{
  try {localStorage.setItem(memoryKey,JSON.stringify(current.current));localStorage.setItem(pendingKey,JSON.stringify(pending.current));}
  catch {if(mounted.current)setError('浏览器无法保留草稿副本，请保持窗口打开并重试同步。')}
 },[]);
 const updateMemory=useCallback((update:(value:StudyMemory)=>StudyMemory)=>{
  const previous=current.current,next=update(previous),stamp=++version.current;
  for(const [id,draft] of Object.entries(next.questions))if(JSON.stringify(draft)!==JSON.stringify(previous.questions[id]))pending.current.questions[id]=stamp;
  if(next.lastId!==previous.lastId)pending.current.lastId=stamp;
  paused.current=false;current.current=next;persist();renderMemory(next);
 },[persist]);
 useEffect(()=>{mounted.current=true;return()=>{mounted.current=false}},[]);
 useEffect(()=>{
  let active=true;ready.current=false;setHydrated(false);
  request<{memory:StudyMemory}>('/api/study-memory').then(({memory:remote})=>{
   if(!active)return;
   if(!remote?.questions||typeof remote.questions!=='object'||Array.isArray(remote.questions))throw Error('学习草稿响应格式无效');
   const local=current.current,questions={...remote.questions};
   for(const [id,draft] of Object.entries(local.questions)) {
    if(pending.current.questions[id]||!(id in questions)) {
     questions[id]=draft;
     pending.current.questions[id] ||= ++version.current;
    }
   }
   const lastId=pending.current.lastId?local.lastId:remote.lastId??local.lastId;
   if(!remote.lastId&&lastId)pending.current.lastId ||= ++version.current;
   current.current={lastId,questions};ready.current=true;setHydrated(true);setError('');persist();renderMemory(current.current);setRevision(n=>n+1);
  }).catch(()=>{if(active)setError('学习草稿尚未同步到本地服务。旧记录不会被覆盖，请重试同步。')});
  return()=>{active=false;ready.current=false};
 },[reload,persist]);
 useEffect(()=>{
  if(!ready.current||inFlight.current||paused.current||(!Object.keys(pending.current.questions).length&&!pending.current.lastId))return;
  const timer=setTimeout(()=>{
   if(inFlight.current||!ready.current)return;
   const snapshot={questions:{...pending.current.questions},lastId:pending.current.lastId};
   const body:StudyMemory={questions:Object.fromEntries(Object.keys(snapshot.questions).map(id=>[id,current.current.questions[id]]))};
   if(snapshot.lastId)body.lastId=current.current.lastId;
   inFlight.current=true;
   request('/api/study-memory',{method:'PUT',body:JSON.stringify(body),keepalive:true}).then(()=>{
    for(const [id,stamp] of Object.entries(snapshot.questions))if(pending.current.questions[id]===stamp)delete pending.current.questions[id];
    if(pending.current.lastId===snapshot.lastId)pending.current.lastId=0;
    persist();if(mounted.current){setError('');setRevision(n=>n+1)}
   }).catch(()=>{paused.current=true;if(mounted.current)setError('学习草稿保存失败，未同步修改仍保留在此浏览器。请重试同步。')})
    .finally(()=>{inFlight.current=false;if(mounted.current)setRevision(n=>n+1)});
  },300);
  return()=>clearTimeout(timer);
 },[memory,revision,persist]);
 const retry=useCallback(()=>{paused.current=false;if(ready.current){setError('');setRevision(n=>n+1)}else setReload(n=>n+1)},[]);
 return {memory,updateMemory,error,retry,hydrated};
}
