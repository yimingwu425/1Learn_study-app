// @vitest-environment jsdom
import React from 'react';
import {afterEach, expect, test, vi} from 'vitest';
import {act, cleanup, fireEvent, render, screen} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {BlueCompanion} from './BlueCompanion';

afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); });

function openPanel() { fireEvent.click(screen.getByRole('button', {name: '打开小蓝学习面板'})); }

test.each(['home', 'study'] as const)('omits the idle caption in %s while preserving the companion and its controls', context => {
  render(<BlueCompanion context={context} actions={[]}/>);
  const trigger = screen.getByRole('button', {name: '打开小蓝学习面板'});
  expect(screen.queryByText('点点小蓝')).toBeNull();
  expect(trigger.querySelector('.blue-companion__caption')).toBeNull();
  expect(screen.getByRole('img', {name: '小蓝'})).toBeTruthy();
  fireEvent.click(trigger);
  expect(screen.getByRole('dialog', {name: '小蓝的学习面板'})).toBeTruthy();
});

test('opens a body portal without stealing focus, waves, runs only enabled actions, and closes', () => {
  const onSelect = vi.fn();
  const disabledSelect = vi.fn();
  const {container} = render(<div style={{overflow: 'hidden'}}><BlueCompanion context="home" size="large" actions={[{id: 'hint', label: '给我一点提示', onSelect}, {id: 'disabled', label: '暂不可用', onSelect: disabledSelect, disabled: true}]}/></div>);
  const trigger = screen.getByRole('button', {name: '打开小蓝学习面板'});
  trigger.focus();
  openPanel();
  const panel = screen.getByRole('dialog', {name: '小蓝的学习面板'});
  expect(panel.parentElement).toBe(document.body);
  expect(container.contains(panel)).toBe(false);
  expect(document.activeElement).toBe(trigger);
  expect(trigger.getAttribute('aria-expanded')).toBe('true');
  expect(screen.getByRole('img', {name: '小蓝'}).getAttribute('data-animation')).toBe('wave');
  fireEvent.click(screen.getByRole('button', {name: '暂不可用'}));
  expect(disabledSelect).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', {name: '给我一点提示'}));
  expect(onSelect).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('supports keyboard opening, panel entry, Escape, repeated toggles, and outside dismissal', async () => {
  const user = userEvent.setup();
  render(<><button>外部按钮</button><BlueCompanion context="study" actions={[]}/></>);
  const trigger = screen.getByRole('button', {name: '打开小蓝学习面板'});
  trigger.focus();
  await user.keyboard('{Enter}');
  expect(screen.getByRole('dialog')).toBeTruthy();
  expect(document.activeElement).toBe(trigger);
  await user.tab();
  expect(document.activeElement).toBe(screen.getByRole('button', {name: '关闭小蓝面板'}));
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(document.activeElement).toBe(trigger);
  await user.keyboard(' ');
  expect(screen.getByRole('dialog')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', {name: '收起小蓝学习面板'}));
  expect(screen.queryByRole('dialog')).toBeNull();
  openPanel();
  fireEvent.pointerDown(screen.getByRole('button', {name: '外部按钮'}));
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('local focus timer runs while hidden, pauses, resumes, finishes honestly, and resets', () => {
  vi.useFakeTimers();
  const fetch = vi.fn();
  vi.stubGlobal('fetch', fetch);
  render(<BlueCompanion context="study" actions={[]}/>);
  openPanel();
  expect(screen.getByRole('timer').textContent).toBe('10:00');
  expect(screen.getByText('收起面板继续计时，离开页面会重置。')).toBeTruthy();
  expect(screen.queryByText('LITTLE STEPS')).toBeNull();
  fireEvent.click(screen.getByRole('button', {name: '开始 10 分钟'}));
  expect(screen.getByText('专注中 10:00')).toBeTruthy();
  expect(screen.getByRole('img', {name: '小蓝'}).getAttribute('data-animation')).toBe('working');
  act(() => vi.advanceTimersByTime(65_000));
  expect(screen.getByRole('timer').textContent).toBe('08:55');
  fireEvent.click(screen.getByRole('button', {name: '暂停'}));
  expect(screen.getByText('已暂停 08:55')).toBeTruthy();
  act(() => vi.advanceTimersByTime(30_000));
  expect(screen.getByRole('timer').textContent).toBe('08:55');
  expect(screen.getByRole('img', {name: '小蓝'}).getAttribute('data-animation')).toBe('waiting');
  fireEvent.click(screen.getByRole('button', {name: '继续专注'}));
  fireEvent.click(screen.getByRole('button', {name: '关闭小蓝面板'}));
  act(() => vi.advanceTimersByTime(60_000));
  openPanel();
  expect(screen.getByRole('timer').textContent).toBe('07:55');
  fireEvent.click(screen.getByRole('button', {name: '关闭小蓝面板'}));
  act(() => vi.advanceTimersByTime(475_000));
  expect(screen.getByText('10 分钟到了')).toBeTruthy();
  expect(screen.getByRole('img', {name: '小蓝'}).getAttribute('data-animation')).toBe('jump');
  expect(screen.getByRole('status').textContent).toContain('10 分钟到了');
  openPanel();
  expect(screen.getByRole('timer').textContent).toBe('00:00');
  expect(screen.getByRole('button', {name: '再来 10 分钟'})).toBeTruthy();
  fireEvent.click(screen.getByRole('button', {name: '重置专注计时'}));
  expect(document.querySelector('.blue-companion__caption')).toBeNull();
  expect(screen.queryByText('点点小蓝')).toBeNull();
  expect(screen.getByRole('timer').textContent).toBe('10:00');
  expect(screen.getByRole('button', {name: '开始 10 分钟'})).toBeTruthy();
  expect(fetch).not.toHaveBeenCalled();
});

test('uses elapsed wall time after backgrounding and keeps the timer across mode changes', () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-11-09T10:00:00Z'));
  const {rerender, unmount} = render(<BlueCompanion context="study" mode="self" actions={[]}/>);
  openPanel();
  fireEvent.click(screen.getByRole('button', {name: '开始 10 分钟'}));
  act(() => vi.advanceTimersByTime(12_000));
  rerender(<BlueCompanion context="study" mode="explain" actions={[]}/>);
  expect(screen.getByRole('timer').textContent).toBe('09:48');
  expect(screen.getByRole('img', {name: '小蓝'}).getAttribute('data-animation')).toBe('working');
  vi.setSystemTime(new Date('2026-11-09T10:11:00Z'));
  fireEvent(document, new Event('visibilitychange'));
  expect(screen.getByRole('timer').textContent).toBe('00:00');
  unmount();
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(vi.getTimerCount()).toBe(0);
  render(<BlueCompanion context="study" actions={[]}/>);
  openPanel();
  expect(screen.getByRole('timer').textContent).toBe('10:00');
});

test('ties sprite reactions to supplied events and current teaching mode, not invented progress', () => {
  vi.useFakeTimers();
  const {rerender} = render(<BlueCompanion context="study" mode="explain" actions={[]}/>);
  const animation = () => screen.getByRole('img', {name: '小蓝'}).getAttribute('data-animation');
  expect(animation()).toBe('review');
  rerender(<BlueCompanion context="study" mode="hint" actions={[]}/>);
  expect(animation()).toBe('waiting');
  rerender(<BlueCompanion context="study" mode="self" event={{id: 1, kind: 'saved'}} actions={[]}/>);
  expect(animation()).toBe('jump');
  expect(screen.getByRole('status').textContent).toBe('已记下这次练习。');
  act(() => vi.advanceTimersByTime(1400));
  expect(animation()).toBe('idle');
  rerender(<BlueCompanion context="study" event={{id: 2, kind: 'retry'}} actions={[]}/>);
  expect(animation()).toBe('waiting');
  expect(screen.getByRole('status').textContent).toContain('还不会也没关系');
  rerender(<BlueCompanion context="study" event={{id: 3, kind: 'failed'}} actions={[]}/>);
  expect(animation()).toBe('waiting');
  expect(screen.getByRole('status').textContent).toContain('这一步还没完成');
});

test('CSS preserves exact idle timing, distinct sprite rows, static reduced motion, and safe mobile placement', async () => {
  const {readFileSync} = await vi.importActual<{readFileSync: (path: string, encoding: 'utf8') => string}>('node:fs');
  const css = readFileSync('src/blue-companion.css', 'utf8');
  expect(css).toContain('25.4545%');
  expect(css).toContain('70.9091%');
  for (const row of ['30%', '40%', '60%', '70%', '80%']) expect(css).toContain(`background-position-y: ${row}`);
  expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*animation: none !important; background-position: 0 0 !important;/);
  expect(css).toContain('position: fixed');
  expect(css).toContain('width: min(320px, calc(100vw - 24px))');
  expect(css).toContain('env(safe-area-inset-bottom)');
});
