// @vitest-environment jsdom
import React from 'react';
import {afterEach,expect,test,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {request} from './api';
import {SourcePreview} from './SourcePreview';
vi.mock('./api',()=>({request:vi.fn()}));
afterEach(()=>{cleanup();vi.resetAllMocks()});
test('a corrected source page recovers after the old indexed page failed',async()=>{
 let finish!:(value:unknown)=>void;
 vi.mocked(request).mockImplementation(path=>path.endsWith('/focus')?new Promise(r=>{finish=r}) as never:Promise.resolve({pages:20}) as never);
 render(<SourcePreview url="/api/files/source#page=99" questionId="q1"/>);
 fireEvent.error(screen.getByRole('img'));
 expect(screen.getByText(/本页暂时无法显示/)).toBeTruthy();
 await act(async()=>finish({located:true,page:14,questionNo:'31',y:100}));
 await waitFor(()=>expect(screen.getByRole('img').getAttribute('src')).toBe('/api/files/source/pages/14'));
 fireEvent.click(screen.getByRole('button',{name:'原卷下一页'}));
 expect(screen.getByRole('img').getAttribute('src')).toBe('/api/files/source/pages/15');
});
