import {useEffect,useRef,useState} from 'react';
import {ChevronLeft,ChevronRight} from 'lucide-react';
import {request} from './api';
export function SourcePreview({url,questionId}:{url:string;questionId?:string}){
 const match=url.match(/^\/api\/files\/([^#?]+)(?:#page=(\d+))?/);
 const base=match?`/api/files/${match[1]}`:'';
 const [page,setPage]=useState(Number(match?.[2]||1)),[pages,setPages]=useState<number|null>(null),[failed,setFailed]=useState(false);
 const [focus,setFocus]=useState<{located:boolean;page?:number;questionNo?:string;y?:number;renderDpi?:number}|null>(null),[loaded,setLoaded]=useState(false),[marker,setMarker]=useState<number|null>(null);
 const viewport=useRef<HTMLDivElement>(null),image=useRef<HTMLImageElement>(null);
 useEffect(()=>{let current=true;if(questionId)request<typeof focus>('/api/questions/'+encodeURIComponent(questionId)+'/focus').then(r=>{if(current){setFocus(r);if(r?.located&&r.page&&Number.isInteger(r.page))setPage(r.page)}}).catch(()=>{});return()=>{current=false}},[questionId]);
 useEffect(()=>{setLoaded(false);setMarker(null);if(viewport.current)viewport.current.scrollTop=0},[page]);
 useEffect(()=>{if(!loaded||!focus?.located||focus.page!==page||focus.y==null)return;const img=image.current;if(!img?.naturalWidth)return;const top=focus.y*(focus.renderDpi||96)/72*img.clientWidth/img.naturalWidth;setMarker(top);if(viewport.current)viewport.current.scrollTop=Math.max(0,top-20)},[focus,loaded,page]);
 useEffect(()=>{let current=true;if(base)request<{pages:number}>(base+'/info').then(r=>{if(current&&Number.isInteger(r.pages))setPages(r.pages)}).catch(()=>{});return()=>{current=false}},[base]);
 return <div className="source-viewer"><div className="source-viewer-toolbar"><span>原卷第 {page}{pages?` / ${pages}`:''} 页{focus?.located&&focus.page===page?` · 已定位第 ${focus.questionNo} 题起始位置`:''}</span><div><button disabled={page<=1} onClick={()=>{setPage(p=>p-1);setFailed(false)}} aria-label="原卷上一页"><ChevronLeft size={16}/></button><button disabled={!pages||page>=pages} onClick={()=>{setPage(p=>p+1);setFailed(false)}} aria-label="原卷下一页"><ChevronRight size={16}/></button></div></div>{base&&!failed?<div className="source-page-viewport" ref={viewport}><div className="source-page-wrap">{marker!==null&&<span className="source-focus-marker" style={{top:Math.max(0,marker-3)}} aria-hidden="true"/>}<img ref={image} className="source-page-image" src={`${base}/pages/${page}`} alt={`原卷第 ${page} 页，保留原始文字与图表`} onLoad={()=>setLoaded(true)} onError={()=>setFailed(true)}/></div></div>:<p className="quiet">本页暂时无法显示，请使用下方兼容预览或打开原文。</p>}<details className="source-fallback"><summary>兼容预览 · 完整原 PDF</summary><iframe src={base?`${base}#page=${page}`:url} title="本题原卷 PDF" className="pdf-preview" loading="lazy"/></details></div>
}
