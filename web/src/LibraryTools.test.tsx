// @vitest-environment jsdom
import React from 'react';
import {afterEach,expect,test,vi} from 'vitest';
import {cleanup,fireEvent,render,screen,within} from '@testing-library/react';
import {CoverageView,ComposerView} from './LibraryTools';
import {request} from './api';
vi.mock('./api',()=>({request:vi.fn()}));
afterEach(()=>{cleanup();vi.clearAllMocks()});
test('chapter ranking retains zero-coverage catalog points and opens the selected real topic',async()=>{
 vi.mocked(request).mockResolvedValue({sample:{questions:10,papers:2,marks:20,yearFirst:2020,yearLast:2026},chapters:[{chapter:1,title:'Mechanics',pointCount:2,seenPointCount:1,questionCount:10,marksTotal:20,marksPct:100},{chapter:2,title:'Waves',pointCount:1,seenPointCount:0,questionCount:0,marksTotal:0,marksPct:0}],points:[{tagId:'A',syllabusRef:'1.1',chapter:1,knowledgePointCn:'牛顿定律',questionCount:10,marksTotal:20,marksPct:100},{tagId:'B',syllabusRef:'1.2',chapter:1,knowledgePointCn:'能量守恒',questionCount:0,marksTotal:0,marksPct:0},{tagId:'C',syllabusRef:'2.1',chapter:2,knowledgePointCn:'波的叠加',questionCount:0,marksTotal:0,marksPct:0}],unclassifiedQuestions:0,basis:'按真实标签统计'});
 const onFind=vi.fn();render(<CoverageView onFind={onFind}/>);
 const table=await screen.findByRole('table');expect(within(table).getAllByRole('row')).toHaveLength(4);
 fireEvent.click(screen.getByRole('button',{name:/^01/ }));expect(within(table).getAllByRole('row')).toHaveLength(3);
 expect(within(table).getByText('能量守恒')).toBeTruthy();
 fireEvent.click(screen.getByRole('button',{name:'查看 牛顿定律 的真题'}));expect(onFind).toHaveBeenCalledWith('9702','牛顿定律','A');
});
test('economics essay papers preserve answer and printed marks and keep the source download',async()=>{
 vi.mocked(request).mockResolvedValue({id:'plan',title:'经济论述练习',actualQuestions:4,actualMarks:40,printedMarks:80,targetMarks:40,exactTarget:true,questions:[{id:'9708:42',paperId:'9708_m23_22',questionRef:'2(a)',marks:20,members:2,topic:'资源配置'}],downloadUrl:'/api/compositions/plan/download'});
 const onOpen=vi.fn();render(<ComposerView onOpen={onOpen}/>);
 fireEvent.change(screen.getByLabelText('学科'),{target:{value:'9708'}});
 fireEvent.change(screen.getByLabelText('卷别'),{target:{value:'paper2essay'}});
 fireEvent.change(screen.getByLabelText('组卷方式'),{target:{value:'paper'}});
 fireEvent.click(screen.getByRole('button',{name:'生成选题方案'}));
 expect(await screen.findByText('4 题组 · 可作答 40 分 · 卷面 80 分')).toBeTruthy();
 const [,options]=vi.mocked(request).mock.calls[0];expect(JSON.parse(options!.body as string)).toMatchObject({subject:'9708',paperType:'paper2essay',mode:'paper'});
 expect(JSON.parse(options!.body as string).targetMarks).toBeUndefined();
 expect(screen.getByRole('link',{name:'导出题面、评分卷和来源'}).getAttribute('href')).toBe('/api/compositions/plan/download');
 fireEvent.click(screen.getByRole('button',{name:'打开组卷题目 9708_m23_22 2(a)'}));expect(onOpen).toHaveBeenCalledWith('9708:42');
});
test('syllabus stays in natural chapter order while the independent ranking uses historical share',async()=>{
 const c=(chapter:number,marks:number)=>({chapter,title:`Chapter ${chapter}`,titleEn:`Chapter ${chapter}`,titleCn:`第 ${chapter} 章`,pointCount:1,seenPointCount:1,questionCount:1,marksTotal:marks,marksPct:marks});
 vi.mocked(request).mockResolvedValue({sample:{questions:3,papers:2,marks:100,yearFirst:2020,yearLast:2026},chapters:[c(2,60),c(11,10),c(1,30)],points:[],unclassifiedQuestions:0,basis:'按真实标签统计'});
 render(<CoverageView onFind={vi.fn()}/>);await screen.findByLabelText('独立历史排名');
 const cards=[...document.querySelectorAll('.chapter-card')].map(c=>c.textContent);
 expect(cards.map(c=>c?.slice(0,2))).toEqual(['01','02','11']);
 const ranking=screen.getByLabelText('独立历史排名');expect(within(ranking).getAllByRole('button').slice(2).map(b=>b.getAttribute('aria-label'))).toEqual(['排名：Chapter 2','排名：Chapter 1','排名：Chapter 11']);
});

test('editing a generated selection retains the original plan for undo',async()=>{
 const q=(id:string)=>({id,paperId:'paper',questionRef:id,marks:1,members:1});
 const original={id:'original',title:'Practice',actualQuestions:2,actualMarks:2,printedMarks:2,targetMarks:2,exactTarget:true,questions:[q('1'),q('2')],downloadUrl:'/api/compositions/original/download'};
 const changed={...original,id:'revised',actualQuestions:1,actualMarks:1,printedMarks:1,exactTarget:false,questions:[q('1')]};
 vi.mocked(request).mockResolvedValueOnce(original).mockResolvedValueOnce(changed);
 render(<ComposerView onOpen={vi.fn()}/>);fireEvent.click(screen.getByRole('button',{name:'生成选题方案'}));
 fireEvent.click(await screen.findByRole('button',{name:'移除组卷题目 paper 2'}));
 expect(await screen.findByText('1 题组 · 可作答 1 分 · 卷面 1 分')).toBeTruthy();
 expect(vi.mocked(request).mock.calls[1][0]).toBe('/api/compositions/original');
 expect(JSON.parse(vi.mocked(request).mock.calls[1][1]!.body as string)).toEqual({action:'remove',index:1});
 fireEvent.click(screen.getByRole('button',{name:'撤销上次选题调整'}));expect(screen.getByText('2 题组 · 可作答 2 分 · 卷面 2 分')).toBeTruthy();
});
