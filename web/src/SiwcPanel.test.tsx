// @vitest-environment jsdom
import React from 'react';
import {test,expect,vi,afterEach} from 'vitest';
import {render,screen,fireEvent,cleanup,waitFor} from '@testing-library/react';
import {SiwcPanel,LunaJobPreview} from './SiwcPanel';
import {request} from './api';
vi.mock('./api',async()=>({...await vi.importActual<typeof import('./api')>('./api'),request:vi.fn()}));
afterEach(()=>{cleanup();vi.clearAllMocks()});
const unsigned={authorized:false,active_registration:null,accounts:[],model:'gpt-6-luna',welcome_needed:false,flow:{phase:'idle'}};
test('does not start login or generate a grant until the user acknowledges the scope and clicks',async()=>{
 vi.mocked(request).mockImplementation(async(path)=>path==='/api/siwc/status'?unsigned:{attempt:'synthetic',browser_url:'http://127.0.0.1/auth/openai?attempt=synthetic'} as never);
 render(<SiwcPanel onChanged={vi.fn()}/>);const login=await screen.findByRole('button',{name:'Continue with ChatGPT'});expect((login as HTMLButtonElement).disabled).toBe(true);expect(request).toHaveBeenCalledTimes(1);
 fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(login);await waitFor(()=>expect(request).toHaveBeenCalledWith('/api/siwc/login/start',{method:'POST',body:JSON.stringify({consent_to_plan_use:true,fresh_registration:false,registration_id:null})}));expect(await screen.findByText('打开官方登录流程')).toBeTruthy();
});
test('requires a live account catalog before enabling a Luna image test and labels accuracy honestly',async()=>{
 const signed={...unsigned,authorized:true,active_registration:'account',accounts:[{id:'account',label:'Synthetic account',signed_in:true,plan_enabled:true}]};
 vi.mocked(request).mockImplementation(async(path)=>{if(path==='/api/siwc/status')return signed as never;if(path==='/api/siwc/models')return {registration_id:'account',models:[{slug:'gpt-6-luna',display_name:'Luna'}],luna_available:true} as never;return {message:'图片测试完成，真题准确率未测量。'} as never});
 render(<SiwcPanel onChanged={vi.fn()}/>);let test=await screen.findByRole('button',{name:'测试 Luna 图片连接'});expect((test as HTMLButtonElement).disabled).toBe(true);fireEvent.click(screen.getByRole('button',{name:'查询账户模型权限'}));await waitFor(()=>expect((screen.getByRole('button',{name:'测试 Luna 图片连接'}) as HTMLButtonElement).disabled).toBe(false));fireEvent.click(screen.getByRole('button',{name:'测试 Luna 图片连接'}));expect(await screen.findByText('图片测试完成，真题准确率未测量。')).toBeTruthy();
});
test('page selection and send consent are explicit; a candidate is never presented as published',async()=>{
 vi.mocked(request).mockResolvedValue({run_id:'run',job_id:'job',can_submit_for_independent_review:true,warnings:['Answer page was not provided'],message:'等待独立复核',proposal:{questions:[{question_no:'31',stem_text:'A real candidate',answer_text:''}]}} as never);
 render(<LunaJobPreview registrationId="account" job={{id:'job',status:'awaiting_agent',sources:[{id:'qp',filename:'question.pdf'},{id:'ms',filename:'mark.pdf'}]}} onChanged={vi.fn()}/>);
 const button=screen.getByRole('button',{name:'用 Luna 识别所选页'});expect((button as HTMLButtonElement).disabled).toBe(true);fireEvent.change(screen.getByLabelText('question.pdf 识别页码'),{target:{value:'2-3'}});fireEvent.change(screen.getByLabelText('mark.pdf 识别页码'),{target:{value:'4'}});fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(button);
 await waitFor(()=>expect(request).toHaveBeenCalledWith('/api/recognition/jobs/job/luna/preview',{method:'POST',body:JSON.stringify({registration_id:'account',pages:[{source_id:'qp',pages:[2,3]},{source_id:'ms',pages:[4]}],question_filter:'',consent_to_send_pages:true})}));expect(await screen.findByText('候选提案 · 尚未复核入库')).toBeTruthy();expect(screen.getByText('A real candidate')).toBeTruthy();expect(request).toHaveBeenCalledTimes(1);
});
test('oversized or malformed page selections are rejected locally without sending any page',async()=>{
 render(<LunaJobPreview registrationId="account" job={{id:'job',status:'awaiting_agent',sources:[{id:'qp',filename:'question.pdf'}]}} onChanged={vi.fn()}/>);fireEvent.change(screen.getByLabelText('question.pdf 识别页码'),{target:{value:'1-8'}});fireEvent.click(screen.getByRole('checkbox'));fireEvent.click(screen.getByRole('button',{name:'用 Luna 识别所选页'}));expect(await screen.findByRole('alert')).toBeTruthy();expect(request).not.toHaveBeenCalled();
});

test('switching the selected registration clears prior send consent',async()=>{
 const job={id:'job',status:'awaiting_agent',sources:[{id:'qp',filename:'question.pdf'}]};const view=render(<LunaJobPreview registrationId="account-a" job={job} onChanged={vi.fn()}/>);fireEvent.click(screen.getByRole('checkbox'));expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(true);view.rerender(<LunaJobPreview registrationId="account-b" job={job} onChanged={vi.fn()}/>);await waitFor(()=>expect((screen.getByRole('checkbox') as HTMLInputElement).checked).toBe(false));expect(request).not.toHaveBeenCalled();
});
