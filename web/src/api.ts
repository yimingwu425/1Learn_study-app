export type AttemptStatus='independent'|'hinted'|'retry';
export interface Question {id:string;subject:string;question_no?:string;qp_pages?:number[];qp_file_id?:string;text_scope?:string;topic?:string;title?:string;text:string;marks?:number;year?:number;source?:string;is_demo?:boolean;topics?:string[];status?:string;kind?:'real'|'demo';qp_url?:string;ms_url?:string;answer?:string;hints?:string[];explanation?:string;tags?:string[];}
export interface Attempt {id?:string;question_id:string;status:AttemptStatus;note:string;created_at?:string;}
export interface Candidate {question_no:string;qp_pages:number[];ms_pages:number[];text:string;marks?:number;topics:string[];}
export interface ImportItem {id:string;filename?:string;name?:string;status:string;question_count?:number;warnings?:string[];subject?:string;exam_code?:string;role?:string;pages?:number;message?:string;candidates?:Candidate[];file_url?:string;}
export async function request<T>(path:string, options:RequestInit={}):Promise<T>{ const r=await fetch(path,{...options,headers:{...(options.body instanceof FormData?{}:{'Content-Type':'application/json'}),...options.headers}}); if(!r.ok){let s=await r.text();try{const detail=JSON.parse(s);if(typeof detail.error==='string')s=detail.error}catch{}throw new Error(s.slice(0,300)||`请求失败 (${r.status})`)}return r.json();}
export const labelStatus=(s:string)=>({independent:'独立做对',hinted:'提示后会了',retry:'还不会'}[s]||s);
export const subjectLabel=(s:string)=>s==='9708'||s.toLowerCase().includes('econ')?'经济':'物理';
export const demo:Question={id:'demo-internal-resistance',subject:'9702',topic:'内阻与电源',title:'电压，去哪了？',text:'A 6.0 V cell has an internal resistance of 1.0 Ω.\nIt is connected to a 5.0 Ω resistor. Find the current.',marks:2,is_demo:true,source:'演示样题 · 非真实试题',hints:['电流不只经过外电阻，也要经过电源内部的电阻。先计算总电阻。','外电阻与内阻串联：R + r = 5.0 + 1.0 = 6.0 Ω。'],explanation:'把整个电路看成一条完整的路径。电源提供的电动势，要克服外电阻和电源自身内阻。\n\nI = E ÷ (R + r) = 6.0 ÷ 6.0 = 1.0 A。\n\n端电压为 IR = 5.0 V，其余 1.0 V 落在内阻上。',answer:'演示答案（非官方评分标准）\nI = E / (R + r)\n= 6.0 / 6.0 = 1.0 A'};
export interface RecognitionStatus {active_registration?:string;provider_configured:boolean;mode:string;message:string;ai_accuracy:null|number;}
export interface RecognitionJob {id:string;status:string;sources:{id:string;filename:string}[];published_count?:number;exceptions?:{question_no?:string;exception_flags:string[]}[];metrics?:Record<string,unknown>;created_at?:string;}
export const jobLabel=(status:string)=>({awaiting_agent:'等待识别 Agent',recognizing:'正在识别题目与答案',awaiting_verification:'等待独立复核',verifying:'正在复核',completed:'处理完成',needs_attention:'部分内容需要关注'}[status]||status);
export const exceptionLabel=(flag:string)=>({missing_answer:'缺少对应答案',unverified_source_quote:'源文内容无法核验',corrupt_text_encoding_requires_vision:'文字层乱码，需要视觉识别',scan_requires_verified_ocr:'扫描页需要经过核验的文字识别',copyright_content_omitted:'原卷因版权删减了内容',independent_verification_required:'尚未完成独立复核',existing_question_will_not_be_overwritten:'题库已有相同题目，不会覆盖',exam_identity_unverified:'试卷身份尚未核实',low_agent_confidence:'识别结果把握不足',multiple_qp_sources:'题面来源存在多个候选',multiple_ms_sources:'答案来源存在多个候选'}[flag]||'需检查：'+flag);

export function questionSummary(q:Question):string {
 if(q.is_demo||q.kind==='demo'||q.status==='agent_verified'||q.status==='reviewed')return q.text.slice(0,150);
 const page=q.qp_pages?.[0]||Number(q.qp_url?.match(/#page=(\d+)/)?.[1]||0);
 const ref=q.question_no||q.title?.split(' · ').at(-1)||'';
 return page?`原卷第 ${page} 页${ref?` · 第 ${ref} 题`:''}。打开查看完整题干与图表。`:'原题页码待核对，请打开原卷按题号定位。';
}
