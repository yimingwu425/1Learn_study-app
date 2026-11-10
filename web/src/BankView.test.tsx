// @vitest-environment jsdom
import React from 'react';
import {test,expect,vi,afterEach} from 'vitest';
import {render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {BankView} from './BankView';
import {request} from './api';
vi.mock('./api',async()=>({...await vi.importActual<typeof import('./api')>('./api'),request:vi.fn()}));
afterEach(()=>{cleanup();vi.clearAllMocks()});
test('loads persisted selections and saves an added question without losing their order',async()=>{
 const a={id:'9702:1',subject:'9702',source:'p',question_no:'31',title:'Question 31',text:'oil droplets from adjacent question',qp_pages:[14],kind:'real' as const};
 const b={...a,id:'9702:2',question_no:'32',title:'Question 32'};
 let selected=[a];
 vi.mocked(request).mockImplementation(async(path,options)=>{
  if(path.startsWith('/api/filter-options'))return {paperIds:['p'],years:['2025'],seasons:['w'],components:['12'],variants:['2'],topics:[],subtopics:[],tags:[]} as never;
  if(path.startsWith('/api/questions?'))return {questions:[a,b],total:2} as never;
  if(path.startsWith('/api/basket?'))return {questions:path.endsWith('9702')?selected:[]} as never;
  if(options?.method==='PUT'){const ids=JSON.parse(options.body as string).ids;selected=ids.map((id:string)=>[a,b].find(q=>q.id===id));return {saved:true} as never;}
  throw Error('unexpected endpoint');
 });
 const props={subject:'9702',query:'',onSubject:vi.fn(),onQuery:vi.fn(),onOpen:vi.fn()};
 const first=render(<BankView {...props}/>);
 expect(await screen.findByText('Question 31')).toBeTruthy();expect(screen.queryByText(/oil droplets/)).toBeNull();
 await waitFor(()=>expect((screen.getByRole('button',{name:'加入试卷篮 Question 32'}) as HTMLButtonElement).disabled).toBe(false));
 fireEvent.click(screen.getByRole('button',{name:'加入试卷篮 Question 32'}));
 await waitFor(()=>expect(selected.map(q=>q.id)).toEqual(['9702:1','9702:2']));
 first.unmount();render(<BankView {...props}/>);fireEvent.click(screen.getByRole('button',{name:/^试卷篮 / }));
 expect(await screen.findByText('2 道题 · 0 分')).toBeTruthy();
 fireEvent.click(screen.getByRole('button',{name:'上移 9702:2'}));
 await waitFor(()=>expect(selected.map(q=>q.id)).toEqual(['9702:2','9702:1']));
});
test('semantic retrieval sends the visible structured filters to the original-vector engine',async()=>{
 const q={id:'9702:1',subject:'9702',text:'',source:'p',question_no:'31',title:'A circuit',qp_pages:[14]};
 vi.mocked(request).mockImplementation(async(path)=>{
  if(path.startsWith('/api/filter-options'))return {paperIds:[],years:[],seasons:[],components:[],variants:[],topics:[],subtopics:[],tags:[]} as never;
  if(path.startsWith('/api/basket'))return {questions:[]} as never;
  if(path==='/api/agent/tools')return {results:[{question:q}],total:1} as never;
  return {questions:[q],total:1} as never;
 });
 render(<BankView subject="9702" query="内阻" onSubject={vi.fn()} onQuery={vi.fn()} onOpen={vi.fn()}/>);
 fireEvent.change(screen.getByLabelText('搜索方式'),{target:{value:'semantic'}});
 fireEvent.click(screen.getByRole('button',{name:'高级筛选'}));
 fireEvent.change(screen.getByLabelText('最高分值'),{target:{value:'2'}});
 await waitFor(()=>expect(vi.mocked(request).mock.calls.some(([path,o])=>path==='/api/agent/tools'&&JSON.parse(o!.body as string).arguments.filters.marksMax==='2')).toBe(true));
 expect(await screen.findByText('A circuit')).toBeTruthy();
});
