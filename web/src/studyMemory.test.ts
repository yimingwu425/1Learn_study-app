// @vitest-environment jsdom
import {act,cleanup,renderHook,waitFor} from '@testing-library/react';
import {afterEach,expect,test,vi} from 'vitest';
import {request} from './api';
import {memoryKey,useStudyMemory} from './studyMemory';
vi.mock('./api',()=>({request:vi.fn()}));
afterEach(()=>{cleanup();localStorage.clear();vi.resetAllMocks()});
const draft=(text:string)=>({mode:'self',hint:0,draft:text});
const sleep=()=>new Promise(resolve=>setTimeout(resolve,380));

test('failed hydration never writes an empty cache; retry retains server drafts',async()=>{
 vi.mocked(request).mockRejectedValueOnce(Error('offline'));
 const {result}=renderHook(()=>useStudyMemory());
 await waitFor(()=>expect(result.current.error).toContain('旧记录不会被覆盖'));
 await act(sleep);expect(request).toHaveBeenCalledTimes(1);
 vi.mocked(request).mockResolvedValueOnce({memory:{lastId:'old',questions:{old:draft('keep me')}}} as never);
 act(()=>result.current.retry());
 await waitFor(()=>expect(result.current.memory.questions.old.draft).toBe('keep me'));
 await act(sleep);expect(vi.mocked(request).mock.calls.every(([,options])=>!options?.method)).toBe(true);
});

test('typing during hydration wins over a delayed server response and writes only dirty entries',async()=>{
 let resolve!:(value:unknown)=>void;
 vi.mocked(request).mockReturnValueOnce(new Promise(r=>{resolve=r}) as never).mockResolvedValue({saved:true} as never);
 const {result}=renderHook(()=>useStudyMemory());
 act(()=>result.current.updateMemory(m=>({...m,lastId:'q1',questions:{...m.questions,q1:draft('new local')}})));
 await act(async()=>resolve({memory:{lastId:'q2',questions:{q1:draft('old remote'),q2:draft('keep remote')}}}));
 expect(result.current.memory.questions.q1.draft).toBe('new local');
 expect(result.current.memory.questions.q2.draft).toBe('keep remote');
 await waitFor(()=>expect(request).toHaveBeenCalledTimes(2));
 const body=JSON.parse(vi.mocked(request).mock.calls[1][1]!.body as string);
 expect(body).toEqual({lastId:'q1',questions:{q1:draft('new local')}});
});

test('a failed write stays pending across remount and requires retry instead of looping',async()=>{
 vi.mocked(request).mockResolvedValueOnce({memory:{questions:{}}} as never).mockRejectedValue(Error('offline'));
 const first=renderHook(()=>useStudyMemory());
 await act(sleep);
 act(()=>first.result.current.updateMemory(m=>({...m,questions:{q1:draft('unsaved')}})));
 await waitFor(()=>expect(first.result.current.error).toContain('保存失败'));
 const count=vi.mocked(request).mock.calls.length;await act(sleep);expect(request).toHaveBeenCalledTimes(count);
 expect(JSON.parse(localStorage.getItem(memoryKey)!).questions.q1.draft).toBe('unsaved');
 first.unmount();
 vi.mocked(request).mockResolvedValueOnce({memory:{questions:{q1:draft('older')}}} as never).mockResolvedValue({saved:true} as never);
 const second=renderHook(()=>useStudyMemory());
 await waitFor(()=>expect(second.result.current.memory.questions.q1.draft).toBe('unsaved'));
 await waitFor(()=>expect(vi.mocked(request).mock.calls.length).toBe(count+2));
});

test('writes are serialized and a later edit survives completion of an earlier save',async()=>{
 let finish!:(value:unknown)=>void;
 vi.mocked(request).mockResolvedValueOnce({memory:{questions:{}}} as never)
  .mockImplementationOnce(()=>new Promise(r=>{finish=r}) as never).mockResolvedValue({saved:true} as never);
 const {result}=renderHook(()=>useStudyMemory());await act(sleep);
 act(()=>result.current.updateMemory(m=>({...m,questions:{q1:draft('first')}})));
 await waitFor(()=>expect(request).toHaveBeenCalledTimes(2));
 act(()=>result.current.updateMemory(m=>({...m,questions:{q1:draft('second')}})));
 await act(sleep);expect(request).toHaveBeenCalledTimes(2);
 await act(async()=>finish({saved:true}));
 await waitFor(()=>expect(request).toHaveBeenCalledTimes(3));
 expect(JSON.parse(vi.mocked(request).mock.calls[2][1]!.body as string).questions.q1.draft).toBe('second');
});
