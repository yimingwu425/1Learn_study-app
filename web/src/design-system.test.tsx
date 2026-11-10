// @vitest-environment jsdom
import React from 'react';
import {afterEach, expect, test, vi} from 'vitest';
import {cleanup, fireEvent, render, screen} from '@testing-library/react';
import {App} from './main';
afterEach(()=>{cleanup();localStorage.clear();document.documentElement.removeAttribute('data-theme');vi.unstubAllGlobals()});
function fixture(){vi.stubGlobal('fetch',vi.fn(async (url:string)=>({ok:true,json:async()=>url.includes('study-memory')?{memory:{questions:{}}}:url.includes('questions')?{questions:[],total:0}:url.includes('attempts')?{attempts:[]}:url.includes('basket')?{questions:[]}:url.includes('recognition/jobs')?{jobs:[]}:url.includes('recognition/status')?{provider_configured:false}:{imports:[]}})))}
test('1mDesign retains identity and selected navigation through the real practice flow',async()=>{
 fixture();render(<App/>);
 expect(screen.getByText('真题学习')).toBeTruthy();
 expect(screen.queryByText('1Learn')).toBeNull();
 expect(screen.getByRole('button',{name:'学习'}).getAttribute('aria-current')).toBe('page');
 fireEvent.click(await screen.findByRole('button',{name:'体验学习桌'}));
 expect(screen.getByRole('button',{name:'真题'}).getAttribute('aria-current')).toBe('page');
 expect(screen.getByRole('button',{name:'自己做'}).getAttribute('aria-pressed')).toBe('true');
 fireEvent.click(screen.getByRole('button',{name:'提示'}));
 expect(screen.getByRole('button',{name:'提示'}).getAttribute('aria-pressed')).toBe('true');
 fireEvent.click(screen.getByRole('button',{name:'记笔记'}));
 fireEvent.change(screen.getByRole('textbox',{name:'我的笔记'}),{target:{value:'An isolated demo draft'}});
 fireEvent.change(screen.getByRole('combobox',{name:'外观'}),{target:{value:'dark'}});
 expect(document.documentElement.dataset.theme).toBe('dark');
 expect((screen.getByRole('textbox',{name:'我的笔记'}) as HTMLTextAreaElement).value).toBe('An isolated demo draft');
 fireEvent.click(screen.getByRole('button',{name:'关闭'}));
 expect(screen.queryByRole('dialog')).toBeNull();
 expect(screen.getByRole('button',{name:'提示'}).getAttribute('aria-pressed')).toBe('true');
 fireEvent.click(screen.getByRole('button',{name:'返回真题'}));
 expect(screen.getByRole('heading',{name:'挑一道，开始学。'})).toBeTruthy();
});
