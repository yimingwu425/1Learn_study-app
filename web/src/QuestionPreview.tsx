import {useEffect,useRef,useState} from 'react';
import {ArrowUpRight,FileText,LoaderCircle} from 'lucide-react';
import {Question,request} from './api';
type Focus={located:boolean;page?:number;y?:number;questionNo?:string;pageWidth?:number;pageHeight?:number;endY?:number};
// Keep lazy source lookups from spawning one PDF engine per result on a page.
let active=0;const queue:(()=>void)[]=[];
async function slot<T>(work:()=>Promise<T>):Promise<T>{if(active>=3)await new Promise<void>(resolve=>queue.push(resolve));active++;try{return await work()}finally{active--;queue.shift()?.()}}
export function QuestionPreview({question:q,onOpen}:{question:Question;onOpen:()=>void}){
 const ref=useRef<HTMLDivElement>(null);const [visible,setVisible]=useState(false),[focus,setFocus]=useState<Focus|null>(null),[ready,setReady]=useState(false),[failed,setFailed]=useState(false);
 const file=q.qp_file_id||q.qp_url?.match(/^\/api\/files\/([^#?]+)/)?.[1];
 const hintedPage=q.qp_pages?.[0]||Number(q.qp_url?.match(/#page=(\d+)/)?.[1]||1);
 const page=focus?.located&&Number.isInteger(focus.page)&&focus.page!>0?focus.page!:hintedPage;
 useEffect(()=>{setReady(false);setFailed(false)},[file,page]);
 useEffect(()=>{const element=ref.current;if(!element)return;if(typeof IntersectionObserver==='undefined'){setVisible(true);return}const observer=new IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting)){setVisible(true);observer.disconnect()}},{rootMargin:'240px'});observer.observe(element);return()=>observer.disconnect()},[q.id]);
 useEffect(()=>{let current=true;if(!visible||!file)return;slot(async()=>{if(!current)return;try{const result=await request<Focus>('/api/questions/'+encodeURIComponent(q.id)+'/focus');if(current)setFocus(result)}catch{if(current)setFocus({located:false})}});return()=>{current=false}},[visible,file,q.id]);
 const w=focus?.pageWidth||0,h=focus?.pageHeight||0;
 const located=!!(focus?.located&&focus.page===page&&focus.y!=null&&w>0&&h>0);
 const top=located?Math.max(0,focus!.y!-12):0;
 const bottom=located&&focus!.endY!=null&&focus!.endY>top+20?Math.min(h,focus!.endY):h;
 const crop=located&&bottom>top+20;
 const verified=q.status==='reviewed'||q.status==='agent_verified'||q.is_demo;
 return <div className="bank-source-preview" ref={ref}>
 {file?<><div className="bank-source-caption"><span><FileText size={13}/>原卷截图 · 第 {page} 页{crop?` · 第 ${q.question_no||focus?.questionNo} 题位置`:focus?' · 题号位置待核对':''}</span><button className="text-button" onClick={onOpen}>展开原卷<ArrowUpRight size={13}/></button></div>
 <div className={'bank-source-window '+(crop?'is-located':'is-full-page')} tabIndex={0} aria-label={`第 ${q.question_no||''} 题原卷截图，可滚动查看`}>
 {!ready&&!failed&&<div className="bank-source-loading"><LoaderCircle size={16}/>{visible?'正在载入真实题面…':'原卷题面预览'}</div>}
 {visible&&!failed&&<div className={crop?'bank-source-crop':'bank-source-full'} style={crop?{paddingBottom:`${(bottom-top)/w*100}%`}:undefined}><img src={`/api/files/${encodeURIComponent(file)}/pages/${page}`} alt={`${q.source||q.subject} 第 ${q.question_no||''} 题所在原卷第 ${page} 页，含原始题干与图表`} loading="lazy" onLoad={()=>setReady(true)} onError={()=>setFailed(true)} style={crop?{transform:`translateY(-${top/h*100}%)`}:undefined}/></div>}
 {failed&&<div className="bank-source-unavailable"><FileText size={19}/><p>原卷图片暂时无法显示</p><button className="text-button" onClick={onOpen}>打开完整原文<ArrowUpRight size={14}/></button></div>}
 </div><p className="bank-source-note">{crop?'题面直接来自原卷。跨页内容和共用材料可展开查看。':'显示原卷页面，可能含相邻题。请按题号核对，或展开原文。'}</p></>:verified&&q.text?<p className="bank-verified-stem">{q.text}</p>:<div className="bank-source-unavailable"><p>原题文件尚未关联，请先核对来源。</p><button className="text-button" onClick={onOpen}>查看来源信息<ArrowUpRight size={14}/></button></div>}
 </div>
}
