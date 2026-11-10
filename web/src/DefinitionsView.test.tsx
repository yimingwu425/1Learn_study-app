// @vitest-environment jsdom
import React from 'react';
import {test,expect,vi,afterEach} from 'vitest';
import {render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {DefinitionsView} from './DefinitionsView';
import {request} from './api';
vi.mock('./api',async()=>({...await vi.importActual<typeof import('./api')>('./api'),request:vi.fn()}));
afterEach(()=>{cleanup();vi.clearAllMocks()});
const source={id:'one',paperId:'9702_m20_22',questionRef:'3(a)',questionId:'real-source',year:2020,qp:{fileId:'qp',page:8,bbox:[25,50,587,180],width:612,height:792},ms:{fileId:'ms',pages:[{page:10,bbox:[25,244,767,268],width:792,height:612,text:'force × displacement in the direction of the force'}]},originalMarkSchemeText:'3(a) force × displacement in the direction of the force B1',answerKind:'written_mark_scheme',questionRequest:'State what is meant by work done.'};
const definition={id:'work',termEn:'Work done',termCn:'做功',syllabusRef:'5.1.1',chapterEn:'5 Work, energy and power',chapterCn:'5 功、能量与功率',reciteEn:'Work done is force multiplied by displacement in the direction of the force.',reciteCn:'功等于力与沿力方向的位移的乘积。',answerLabel:'整理',scopeNote:'定义得分点',preferredOccurrenceId:'one',occurrenceCount:2,paperCount:2,occurrences:[source],trend:[{year:2020,count:2}]};
const catalogue={definitions:[definition],total:1,offset:0,limit:8,chapters:[{chapter:5,titleEn:'5 Work, energy and power',titleCn:'5 功、能量与功率'}],ranking:[{id:'work',termEn:'Work done',termCn:'做功',count:2,papers:2}],trend:[{year:2020,count:2}],sample:{terms:1,occurrences:2,papers:2,scannedPapers:175,sourceScanTotal:271,unavailableSourceRecords:0},scope:'可读文字已核对样本，非全库穷尽识别',dedupRule:'同一术语同一原卷小问只计一次',frequencyInterpretation:'未收录不等于没考过'};
test('shows bilingual editorial answers, independent source wording and a genuine source image',async()=>{
 vi.mocked(request).mockResolvedValue(catalogue as never);const open=vi.fn();render(<DefinitionsView onOpen={open}/>);
 expect(await screen.findByText(definition.reciteEn)).toBeTruthy();expect(screen.getByText(definition.reciteCn)).toBeTruthy();expect(screen.getByText('背诵版 · 据评分要求整理')).toBeTruthy();expect(screen.queryByRole('img')).toBeNull();
 fireEvent.click(screen.getByRole('button',{name:'核对评分标准与原题'}));expect(await screen.findByRole('img')).toHaveProperty('src','http://localhost:3000/api/files/ms/pages/10');expect(screen.getByText(source.originalMarkSchemeText)).toBeTruthy();fireEvent.click(screen.getByRole('button',{name:'去看这道真题'}));expect(open).toHaveBeenCalledWith('real-source');
 fireEvent.click(screen.getByRole('button',{name:'遮住答案'}));expect(screen.queryByText(definition.reciteEn)).toBeNull();expect(screen.getByText(source.originalMarkSchemeText)).toBeTruthy();
});
test('subject, chapter and year filters are sent to the source-backed endpoint',async()=>{
 vi.mocked(request).mockResolvedValue(catalogue as never);render(<DefinitionsView onOpen={vi.fn()}/>);await screen.findByText(definition.reciteEn);fireEvent.change(screen.getByLabelText('定义库章节'),{target:{value:'5'}});fireEvent.change(screen.getByLabelText('定义库起始年份'),{target:{value:'2021'}});fireEvent.change(screen.getByLabelText('定义库排序'),{target:{value:'frequency'}});
 await waitFor(()=>expect(vi.mocked(request).mock.calls.some(([url])=>url.includes('chapter=5')&&url.includes('year_from=2021')&&url.includes('sort=frequency'))).toBe(true));
});
test('MCQ official key is distinguished from the definition quoted from the selected option',async()=>{
 vi.mocked(request).mockResolvedValue({...catalogue,definitions:[{...definition,occurrences:[{...source,answerKind:'correct_mcq_option',correctOption:'A',correctOptionText:'energy transferred per unit charge'}]}]} as never);render(<DefinitionsView onOpen={vi.fn()}/>);fireEvent.click(await screen.findByRole('button',{name:'核对评分标准与原题'}));expect(screen.getByText(/评分表本身只给答案字母/)).toBeTruthy();expect(screen.getByText('energy transferred per unit charge')).toBeTruthy();expect(screen.queryByText(source.originalMarkSchemeText)).toBeNull();
});
test('empty coverage is labelled uncollected, never asserted to be absent from exams',async()=>{
 vi.mocked(request).mockResolvedValue({...catalogue,definitions:[],total:0,ranking:[],trend:[],sample:{...catalogue.sample,terms:0,occurrences:0,papers:0}} as never);render(<DefinitionsView onOpen={vi.fn()}/>);expect(await screen.findByText('这个范围暂未收录已核对定义。')).toBeTruthy();expect(screen.getByText(/尚未收录不等于该范围没有考过定义/)).toBeTruthy();
});
