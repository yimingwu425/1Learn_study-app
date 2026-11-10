// @vitest-environment jsdom
import React from 'react';
import {test,expect,vi,afterEach} from 'vitest';
import {render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {QuestionPreview} from './QuestionPreview';
import {request} from './api';
vi.mock('./api',async()=>({...await vi.importActual<typeof import('./api')>('./api'),request:vi.fn()}));
afterEach(()=>{cleanup();vi.clearAllMocks();vi.unstubAllGlobals()});
const question={id:'9702:10000',subject:'9702',source:'9702_w23_12',question_no:'31',qp_file_id:'d81f4af8-192f-5d5a-887a-918d79f09ffd',qp_pages:[14],status:'legacy_unreviewed',text:'oil droplets from adjacent question'};
test('displays the real source image at the independently located question, never the corrupt excerpt',async()=>{
 vi.stubGlobal('IntersectionObserver',undefined);
 vi.mocked(request).mockResolvedValue({located:true,page:14,y:217.0443,pageWidth:612,pageHeight:792,endY:428.5646} as never);
 const onOpen=vi.fn();render(<QuestionPreview question={question} onOpen={onOpen}/>);
 const image=await screen.findByRole('img');expect(image.getAttribute('src')).toBe('/api/files/d81f4af8-192f-5d5a-887a-918d79f09ffd/pages/14');
 await waitFor(()=>expect(screen.getByText(/第 31 题位置/)).toBeTruthy());
 expect(image.style.transform).toContain('translateY(-25.');expect(screen.queryByText(/oil droplets/)).toBeNull();
 fireEvent.load(image);expect(screen.queryByText('正在载入真实题面…')).toBeNull();
 fireEvent.click(screen.getByRole('button',{name:'展开原卷'}));expect(onOpen).toHaveBeenCalledOnce();
});
test('uncertain position keeps a scrollable whole source page and offers the original on image failure',async()=>{
 vi.stubGlobal('IntersectionObserver',undefined);vi.mocked(request).mockResolvedValue({located:false} as never);
 const onOpen=vi.fn();render(<QuestionPreview question={{...question,id:'other',question_no:'32'}} onOpen={onOpen}/>);
 const image=await screen.findByRole('img');await screen.findByText(/题号位置待核对/);expect(image.style.transform).toBe('');expect(screen.queryByText(/oil droplets/)).toBeNull();
 fireEvent.error(image);expect(await screen.findByText('原卷图片暂时无法显示')).toBeTruthy();fireEvent.click(screen.getByRole('button',{name:'打开完整原文'}));expect(onOpen).toHaveBeenCalledOnce();
});
test('offscreen entries do not start PDF lookups or render images until visible',async()=>{
 let show:()=>void=()=>{};
 vi.stubGlobal('IntersectionObserver',class{constructor(callback:(entries:{isIntersecting:boolean}[])=>void){show=()=>callback([{isIntersecting:true}])}observe(){}disconnect(){}});
 vi.mocked(request).mockResolvedValue({located:false} as never);render(<QuestionPreview question={question} onOpen={vi.fn()}/>);
 expect(request).not.toHaveBeenCalled();expect(screen.queryByRole('img')).toBeNull();
 show();await waitFor(()=>expect(request).toHaveBeenCalledWith('/api/questions/9702%3A10000/focus'));expect(await screen.findByRole('img')).toBeTruthy();
});

test('uses a source-verified page correction for the image without changing question metadata',async()=>{
 vi.stubGlobal('IntersectionObserver',undefined);vi.mocked(request).mockResolvedValue({located:true,page:4,y:60,pageWidth:612,pageHeight:792,endY:300} as never);
 const q={...question,qp_pages:[3]};render(<QuestionPreview question={q} onOpen={vi.fn()}/>);
 await waitFor(()=>expect(screen.getByRole('img').getAttribute('src')).toContain('/pages/4'));
 expect(screen.getByText(/原卷截图 · 第 4 页/)).toBeTruthy();expect(q.qp_pages).toEqual([3]);
});

test('a corrected source page can load after the old indexed page failed',async()=>{
 vi.stubGlobal('IntersectionObserver',undefined);
 let complete!:(value:unknown)=>void;
 vi.mocked(request).mockReturnValue(new Promise(resolve=>{complete=resolve}) as never);
 render(<QuestionPreview question={{...question,qp_pages:[99]}} onOpen={vi.fn()}/>);
 const oldImage=await screen.findByRole('img');fireEvent.error(oldImage);
 expect(await screen.findByText('原卷图片暂时无法显示')).toBeTruthy();
 complete({located:true,page:14,y:217,pageWidth:612,pageHeight:792,endY:429});
 await waitFor(()=>expect(screen.getByRole('img').getAttribute('src')).toContain('/pages/14'));
 fireEvent.load(screen.getByRole('img'));expect(screen.queryByText('原卷图片暂时无法显示')).toBeNull();
});
