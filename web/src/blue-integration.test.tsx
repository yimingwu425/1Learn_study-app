// @vitest-environment jsdom
import React from 'react';
import {test,expect,vi,afterEach} from 'vitest';
import {render,screen,fireEvent,cleanup} from '@testing-library/react';
import {App} from './main';
afterEach(()=>{cleanup();localStorage.clear();vi.unstubAllGlobals()});
const clickPet=()=>fireEvent.click(screen.getByRole('img',{name:'小蓝'}).closest('button')!);
function mockEmptyLibrary(){
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>({ok:true,json:async()=>url.includes('questions')?{questions:[],total:0}:url.includes('attempts')?{attempts:[]}:url.includes('recognition/jobs')?{jobs:[]}:{imports:[]}})));
}
test('Xiaolan opens a real study flow, prepared hint, and note dialog',async()=>{
 mockEmptyLibrary();render(<App/>);await screen.findByRole('button',{name:'体验学习桌'});
 clickPet();fireEvent.click(await screen.findByRole('button',{name:'打开演示题'}));
 expect(screen.getByRole('heading',{name:'电压，去哪了？'})).toBeTruthy();
 clickPet();fireEvent.click(await screen.findByRole('button',{name:'打开已有提示'}));
 expect(screen.getByRole('button',{name:'提示'}).className).toBe('on');
 expect(screen.getAllByText(/总电阻/).length).toBeGreaterThan(0);
 clickPet();fireEvent.click(await screen.findByRole('button',{name:'记下卡住的地方'}));
 const note=screen.getByRole('textbox',{name:'我的笔记'});
 fireEvent.change(note,{target:{value:'用小蓝记住了内阻'}});
 fireEvent.click(screen.getByRole('button',{name:'独立做对'}));
 expect(await screen.findByText('演示记录仅保留在本次会话')).toBeTruthy();
 fireEvent.click(screen.getByRole('button',{name:'我的资料'}));
 expect(await screen.findByText('用小蓝记住了内阻')).toBeTruthy();
});
test('Xiaolan uses official answer action instead of inventing a missing hint',async()=>{
 const q={id:'no-hint',title:'原题',text:'Real source text',subject:'9702',kind:'real',answer:'已有评分标准'};
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>({ok:true,json:async()=>url.includes('questions')?{questions:[q],total:1}:url.includes('attempts')?{attempts:[]}:{imports:[]}})));
 render(<App/>);fireEvent.click(await screen.findByRole('button',{name:'打开第一道真题'}));
 clickPet();expect(screen.queryByRole('button',{name:'打开已有提示'})).toBeNull();
 fireEvent.click(await screen.findByRole('button',{name:'对照评分标准'}));
 expect(screen.getByRole('dialog',{name:'评分标准'})).toBeTruthy();
 expect(screen.getByText('已有评分标准')).toBeTruthy();
});
