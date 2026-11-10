// @vitest-environment jsdom
import React from 'react';
import {test,expect,vi,afterEach} from 'vitest';
import {act,render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {App} from './main';
afterEach(()=>{cleanup();localStorage.clear();vi.unstubAllGlobals()});
test('empty real library does not invent records; demo is explicit',async()=>{
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>({ok:true,json:async()=>url.includes('questions')?{questions:[]}:url.includes('attempts')?{attempts:[]}:{imports:[]}})));
 render(<App/>);
 const demo=await screen.findByText('体验学习桌');fireEvent.click(demo);
 expect(screen.getAllByText(/演示/).length).toBeGreaterThan(0);
 fireEvent.click(screen.getByText(/给我一点提示/));
 expect(screen.getAllByText(/总电阻/).length).toBeGreaterThan(0);
});
test('unknown PDF can be reviewed with explicit exam code',async()=>{
 const record={id:'upload-1',filename:'unknown.pdf',status:'needs_review',role:'unknown',pages:2,file_url:'/api/files/upload-1',candidates:[]};
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>({ok:true,json:async()=>url.includes('questions')?{questions:[]}:url.includes('attempts')?{attempts:[]}:{imports:[record]}})));
 render(<App/>);fireEvent.click(screen.getByRole('button',{name:'导入题目'}));
 fireEvent.click(screen.getByText('手动兜底 / 调整原件组合'));fireEvent.click(await screen.findByRole('button',{name:/unknown.pdf/}));
 await waitFor(()=>expect(screen.getByLabelText('试卷代码')).toBeTruthy());
 expect((screen.getByRole('button',{name:'我已核对，加入题库'}) as HTMLButtonElement).disabled).toBe(true);
});

test('resumes the last question and mode; embeds original PDF and isolates drafts',async()=>{
 const qs=[{id:'q1',title:'Question One',text:'First question',subject:'9702',kind:'real',qp_url:'/api/files/one#page=2'},{id:'q2',title:'Question Two',text:'Second question',subject:'9702',kind:'real',qp_url:'/api/files/two#page=3'}];
 const fetchMock=vi.fn(async(url:string)=>({ok:true,json:async()=>url.includes('questions')?{questions:qs}:url.includes('attempts')?{attempts:[]}:{imports:[]}}));
 vi.stubGlobal('fetch',fetchMock);
 localStorage.setItem('study-app-v2.learning',JSON.stringify({lastId:'q2',questions:{q2:{mode:'explain',hint:0,draft:'second-note'}}}));
 const first=render(<App/>);
 fireEvent.click(await screen.findByRole('button',{name:'继续学习'}));
 expect(screen.getByRole('button',{name:'讲解'}).className).toBe('on');
 expect(screen.getByTitle('本题原卷 PDF').getAttribute('src')).toBe('/api/files/two#page=3');
 expect(screen.getByText(/可能包含相邻题/)).toBeTruthy();
 fireEvent.click(screen.getByRole('button',{name:'记笔记'}));
 expect((screen.getByRole('textbox',{name:'我的笔记'}) as HTMLTextAreaElement).value).toBe('second-note');
 fireEvent.change(screen.getByRole('textbox',{name:'我的笔记'}),{target:{value:'updated-second'}});
 fireEvent.click(screen.getByRole('button',{name:'关闭'}));
 fireEvent.click(screen.getByRole('button',{name:'返回真题'}));
 fireEvent.click(await screen.findByText('Question One'));
 fireEvent.click(screen.getByRole('button',{name:'记笔记'}));
 expect((screen.getByRole('textbox',{name:'我的笔记'}) as HTMLTextAreaElement).value).toBe('');
 fireEvent.change(screen.getByRole('textbox',{name:'我的笔记'}),{target:{value:'first-note'}});
 first.unmount();
 render(<App/>);
 fireEvent.click(await screen.findByRole('button',{name:'继续学习'}));
 expect(screen.getByTitle('本题原卷 PDF').getAttribute('src')).toBe('/api/files/one#page=2');
 fireEvent.click(screen.getByRole('button',{name:'记笔记'}));
 expect((screen.getByRole('textbox',{name:'我的笔记'}) as HTMLTextAreaElement).value).toBe('first-note');
});
test('searches the full server library, paginates, and restores a question beyond page one',async()=>{
 const q={id:'archive:7001',title:'Beyond page one',text:'Question beyond first page',subject:'9702',kind:'real',qp_url:'/api/files/archive#page=4'};
 localStorage.setItem('study-app-v2.learning',JSON.stringify({lastId:q.id,questions:{}}));
 const calls:string[]=[];
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>{
  calls.push(url);
  let body:unknown={};
  if(url.startsWith('/api/questions/'))body=q;
  else if(url.startsWith('/api/questions?')){
   const params=new URLSearchParams(url.split('?')[1]);
   body=params.get('limit')==='6'?{questions:[],total:7001}:params.get('query')==='absent'?{questions:[],total:0}:{questions:[{...q,title:params.get('offset')==='30'?'Second page question':'First page question'}],total:7001};
  }else body=url.includes('attempts')?{attempts:[]}:{imports:[]};
  return {ok:true,json:async()=>body}
 }));
 render(<App/>);
 fireEvent.click(await screen.findByRole('button',{name:'继续学习'}));
 expect(calls).toContain('/api/questions/archive%3A7001');
 expect(screen.getByText('Beyond page one')).toBeTruthy();
 fireEvent.click(screen.getByRole('button',{name:'返回真题'}));
 expect(await screen.findByText('First page question')).toBeTruthy();
 expect(screen.getByText(/共 7001 题/)).toBeTruthy();
 fireEvent.click(screen.getByRole('button',{name:'下一页'}));
 expect(await screen.findByText('Second page question')).toBeTruthy();
 expect(calls.some(u=>u.includes('offset=30')&&u.includes('limit=30'))).toBe(true);
 fireEvent.change(screen.getByRole('textbox',{name:'筛选题目'}),{target:{value:'absent'}});
 fireEvent.change(screen.getByRole('combobox',{name:'学科'}),{target:{value:'9708'}});
 expect(await screen.findByText('没有匹配的题目')).toBeTruthy();
 expect(screen.queryByText('题库还没有题目')).toBeNull();
 expect(calls.some(u=>u.includes('query=absent')&&u.includes('subject=9708')&&u.includes('offset=0'))).toBe(true);
});
test('unknown file can be classified as MS before pairing',async()=>{
 const item={id:'unknown-ms',filename:'answers.pdf',status:'needs_review',role:'unknown',pages:1,candidates:[]};
 const calls:{url:string;options?:RequestInit}[]=[];
 vi.stubGlobal('fetch',vi.fn(async(url:string,options?:RequestInit)=>{
  calls.push({url,options});
  return {ok:true,json:async()=>url.includes('questions')?{questions:[],total:0}:url.includes('attempts')?{attempts:[]}:{imports:[item]}};
 }));
 render(<App/>);
 fireEvent.click(screen.getByRole('button',{name:'导入题目'}));
 fireEvent.click(screen.getByText('手动兜底 / 调整原件组合'));fireEvent.click(await screen.findByRole('button',{name:/answers.pdf/}));
 fireEvent.change(screen.getByLabelText('文件类型'),{target:{value:'ms'}});
 fireEvent.change(screen.getByLabelText('试卷代码'),{target:{value:'9702_s24_22'}});
 fireEvent.click(screen.getByRole('button',{name:'保存基本信息'}));
 await waitFor(()=>expect(calls.some(c=>c.options?.method==='PATCH')).toBe(true));
 const call=calls.find(c=>c.options?.method==='PATCH')!;
 expect(call.url).toBe('/api/imports/unknown-ms');
 expect(JSON.parse(call.options!.body as string)).toEqual({subject:'9702',exam_code:'9702_s24_22',role:'ms'});
 expect(screen.queryByRole('button',{name:'我已核对，加入题库'})).toBeNull();
});

test('recognition jobs lead the import view; manual review stays secondary',async()=>{
 const flags=['missing_answer','unverified_source_quote','corrupt_text_encoding_requires_vision','copyright_content_omitted','independent_verification_required','existing_question_will_not_be_overwritten'];
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>({ok:true,json:async()=>url==='/api/recognition/status'?{provider_configured:false,mode:'external_agent_pull',message:'等待接入识别 Agent',ai_accuracy:null}:url==='/api/recognition/jobs'?{jobs:[{id:'job1',status:'awaiting_agent',sources:[{id:'source1',filename:'original.pdf'}],published_count:0,exceptions:[]},{id:'job2',status:'needs_attention',sources:[],published_count:8,exceptions:[{question_no:'2',exception_flags:flags}]}]}:url.includes('questions')?{questions:[],total:0}:url.includes('attempts')?{attempts:[]}:{imports:[]}})));
 render(<App/>);fireEvent.click(screen.getByRole('button',{name:'导入题目'}));
 expect(await screen.findByText('等待识别 Agent')).toBeTruthy();
 expect(screen.getByText(/现在不需要逐题配对/)).toBeTruthy();
 expect(screen.getByText('8 题已入库')).toBeTruthy();
 expect(screen.getByText('手动兜底 / 调整原件组合').closest('details')?.open).toBe(false);
 fireEvent.click(screen.getByText('1 处异常待查'));
 for(const label of ['缺少对应答案','源文内容无法核验','文字层乱码，需要视觉识别','原卷因版权删减了内容','尚未完成独立复核','题库已有相同题目，不会覆盖'])expect(screen.getByText(label)).toBeTruthy();
});


test('home omits the removed slogans without substituting new copy',async()=>{
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>({ok:true,json:async()=>url.includes('questions')?{questions:[],total:0}:url.includes('attempts')?{attempts:[]}:{imports:[]}})));
 render(<App/>);
 await screen.findByRole('button',{name:'体验学习桌'});
 for(const copy of ['今天，再懂一点','YOUR LEARNING SPACE','从一道题开始。把不懂的，慢慢变成自己的。','按已有题目和考点检索 · 不会把关键词搜索冒充 AI 导师'])expect(screen.queryByText(copy)).toBeNull();
 expect(screen.getByRole('heading',{name:'今天，想弄懂什么？'})).toBeTruthy();
 expect(document.querySelector('.breadcrumb')?.textContent).toBe('');
 expect(document.querySelector('.hero > .eyebrow')).toBeNull();
 expect(document.querySelector('.hero > p')).toBeNull();
 expect(document.querySelector('.search-note')).toBeNull();
 expect(screen.getByRole('img',{name:'小蓝'})).toBeTruthy();
 expect(document.querySelector('.mark-orbit')).toBeNull();
});

test('opening a question before draft hydration does not replace its unseen note with an empty draft',async()=>{
 const q={id:'q-late',title:'Late notes',text:'Question',subject:'9702',kind:'real'};
 let finish!:(value:unknown)=>void;
 const calls:{url:string;options?:RequestInit}[]=[];
 vi.stubGlobal('fetch',vi.fn(async(url:string,options?:RequestInit)=>{
  calls.push({url,options});
  if(url==='/api/study-memory'&&!options?.method)return {ok:true,json:()=>new Promise(resolve=>{finish=resolve})};
  return {ok:true,json:async()=>url.includes('questions')?{questions:[q],total:1}:url.includes('attempts')?{attempts:[]}:url.includes('imports')?{imports:[]}:{saved:true}};
 }));
 render(<App/>);
 fireEvent.click(await screen.findByRole('button',{name:'打开第一道真题'}));
 fireEvent.click(screen.getByRole('button',{name:'记笔记'}));
 finish({memory:{lastId:q.id,questions:{[q.id]:{mode:'self',hint:0,draft:'Previously saved note'}}}});
 await waitFor(()=>expect((screen.getByRole('textbox',{name:'我的笔记'}) as HTMLTextAreaElement).value).toBe('Previously saved note'));
 await act(()=>new Promise(resolve=>setTimeout(resolve,380)));
 const writes=calls.filter(c=>c.url==='/api/study-memory'&&c.options?.method==='PUT').map(c=>JSON.parse(c.options!.body as string));
 expect(writes.every(w=>!w.questions[q.id]||w.questions[q.id].draft==='Previously saved note')).toBe(true);
});
