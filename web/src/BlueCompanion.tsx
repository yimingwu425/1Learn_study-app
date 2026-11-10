import {useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent} from 'react';
import {createPortal} from 'react-dom';
import {ArrowUpRight, Clock3, Pause, Play, RotateCcw, X} from 'lucide-react';
import './blue-companion.css';

export type BlueCompanionAction = {
  id: string;
  label: string;
  onSelect: () => void;
  disabled?: boolean;
};

export type BlueCompanionEvent = {id: number; kind: 'saved' | 'retry' | 'failed'};

export type BlueCompanionProps = {
  context: 'home' | 'study';
  mode?: 'self' | 'hint' | 'explain';
  event?: BlueCompanionEvent;
  actions: BlueCompanionAction[];
  size?: 'large' | 'normal';
};

type Animation = 'idle' | 'wave' | 'jump' | 'waiting' | 'working' | 'review';
type TimerState = 'idle' | 'running' | 'paused' | 'finished';
const FOCUS_DURATION = 10 * 60 * 1000;

/** Local-only timer: closing the panel preserves it; unmounting the companion resets it. */
export function BlueCompanion({context, mode = 'self', event, actions, size = 'normal'}: BlueCompanionProps) {
  const panelId = useId();
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<CSSProperties>({left: 12, top: 12});
  const [reaction, setReaction] = useState<{animation: Animation; serial: number} | null>(null);
  const [timerState, setTimerState] = useState<TimerState>('idle');
  const [remainingMs, setRemainingMs] = useState(FOCUS_DURATION);
  const [notice, setNotice] = useState('');
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const deadlineRef = useRef<number | null>(null);
  const serialRef = useRef(0);

  function animate(animation: Animation) {
    setReaction({animation, serial: ++serialRef.current});
  }

  useEffect(() => {
    if (!reaction) return;
    const timeout = window.setTimeout(() => setReaction(null), reaction.animation === 'waiting' ? 2600 : 1400);
    return () => window.clearTimeout(timeout);
  }, [reaction]);

  useEffect(() => {
    if (!event) return;
    setReaction({animation: event.kind === 'saved' ? 'jump' : 'waiting', serial: ++serialRef.current});
    setNotice(event.kind === 'saved' ? '已记下这次练习。' : event.kind === 'retry' ? '还不会也没关系，再看一小步。' : '这一步还没完成，可以再试一次。');
    const timeout = window.setTimeout(() => setNotice(''), 5000);
    return () => window.clearTimeout(timeout);
  }, [event?.id, event?.kind]);

  useEffect(() => {
    if (timerState !== 'running') return;
    const tick = () => {
      const remaining = Math.max(0, (deadlineRef.current ?? Date.now()) - Date.now());
      setRemainingMs(remaining);
      if (remaining === 0) {
        deadlineRef.current = null;
        setTimerState('finished');
        setReaction({animation: 'jump', serial: ++serialRef.current});
        setNotice('10 分钟到了，可以停下来伸个懒腰。');
      }
    };
    const interval = window.setInterval(tick, 1000);
    // Use the deadline rather than counting ticks, including after a backgrounded tab resumes.
    document.addEventListener('visibilitychange', tick);
    window.addEventListener('focus', tick);
    return () => {
      window.clearInterval(interval);
      document.removeEventListener('visibilitychange', tick);
      window.removeEventListener('focus', tick);
    };
  }, [timerState]);

  function closePanel(restoreFocus = false) {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  }

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      closePanel(!!panelRef.current?.contains(document.activeElement));
    };
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as Node;
      if (!panelRef.current?.contains(target) && !triggerRef.current?.contains(target)) closePanel();
    };
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('pointerdown', onPointerDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.removeEventListener('pointerdown', onPointerDown);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (!open) return;
    const placePanel = () => {
      const trigger = triggerRef.current;
      const panel = panelRef.current;
      if (!trigger || !panel) return;
      const rect = trigger.getBoundingClientRect();
      const width = panel.offsetWidth || Math.min(320, window.innerWidth - 24);
      const height = panel.offsetHeight;
      const left = Math.max(12, Math.min(rect.left + rect.width / 2 - width / 2, window.innerWidth - width - 12));
      const below = rect.bottom + 10;
      const top = Math.max(12, Math.min(below + height <= window.innerHeight - 12 ? below : rect.top - height - 10, window.innerHeight - height - 12));
      setPosition({left, top});
    };
    placePanel();
    window.addEventListener('resize', placePanel);
    window.addEventListener('scroll', placePanel, true);
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(placePanel);
    if (panelRef.current) observer?.observe(panelRef.current);
    return () => {
      window.removeEventListener('resize', placePanel);
      window.removeEventListener('scroll', placePanel, true);
      observer?.disconnect();
    };
  }, [open]);

  function togglePanel() {
    animate('wave');
    setOpen(value => !value);
  }

  function startTimer() {
    const duration = timerState === 'paused' ? remainingMs : FOCUS_DURATION;
    deadlineRef.current = Date.now() + duration;
    setRemainingMs(duration);
    setReaction(null);
    setNotice('');
    setTimerState('running');
  }

  function pauseTimer() {
    const remaining = Math.max(0, (deadlineRef.current ?? Date.now()) - Date.now());
    deadlineRef.current = null;
    setRemainingMs(remaining);
    setTimerState(remaining === 0 ? 'finished' : 'paused');
    if (remaining === 0) {
      animate('jump');
      setNotice('10 分钟到了，可以停下来伸个懒腰。');
    }
  }

  function resetTimer() {
    deadlineRef.current = null;
    setRemainingMs(FOCUS_DURATION);
    setTimerState('idle');
    setReaction(null);
    setNotice('');
  }

  function triggerKeyDown(e: KeyboardEvent<HTMLButtonElement>) {
    // A portal is last in the DOM; let keyboard users enter it directly without moving focus on open.
    if (open && ((e.key === 'Tab' && !e.shiftKey) || e.key === 'ArrowDown')) {
      e.preventDefault();
      panelRef.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
    }
  }

  const animation: Animation = reaction?.animation ?? (timerState === 'running' ? 'working' : timerState === 'paused' ? 'waiting' : mode === 'explain' ? 'review' : mode === 'hint' ? 'waiting' : 'idle');
  const seconds = Math.ceil(remainingMs / 1000);
  const time = `${Math.floor(seconds / 60).toString().padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`;
  const timerLabel = timerState === 'running' ? '正在专注' : timerState === 'paused' ? '已暂停' : timerState === 'finished' ? '本次计时结束' : '给自己 10 分钟';
  const subtitle = context === 'home' ? '选一个小目标，慢慢来。' : mode === 'explain' ? '对照已有讲解，理一理思路。' : mode === 'hint' ? '先看一小步，不用急着会。' : '按自己的节奏，先试一试。';

  return <div className={`blue-companion blue-companion--${size}`} data-context={context}>
    <button ref={triggerRef} type="button" className="blue-companion__trigger" aria-label={open ? '收起小蓝学习面板' : '打开小蓝学习面板'} aria-expanded={open} aria-controls={open ? panelId : undefined} aria-haspopup="dialog" onClick={togglePanel} onKeyDown={triggerKeyDown}>
      <span key={reaction?.serial ?? animation} className={`assistant-mark blue-companion__sprite blue-companion__sprite--${animation}`} role="img" aria-label="小蓝" data-animation={animation}/>
      {timerState !== 'idle' && <span className="blue-companion__caption">{timerState === 'running' ? `专注中 ${time}` : timerState === 'paused' ? `已暂停 ${time}` : '10 分钟到了'}</span>}
    </button>
    <span className="blue-companion__sr-only" role="status" aria-live="polite">{notice}</span>
    {open && createPortal(<section ref={panelRef} id={panelId} role="dialog" aria-label="小蓝的学习面板" className="blue-companion-panel" style={position} onKeyDown={e => {
      if (e.key === 'Tab' && e.shiftKey && document.activeElement === panelRef.current?.querySelector('button:not(:disabled)')) {
        e.preventDefault();
        triggerRef.current?.focus();
      }
    }}>
      <header className="blue-companion-panel__header">
        <div><h2>和小蓝一起</h2></div>
        <button type="button" className="blue-companion-panel__close" aria-label="关闭小蓝面板" onClick={() => closePanel(true)}><X size={18} aria-hidden="true"/></button>
      </header>
      <p className="blue-companion-panel__subtitle">{notice || subtitle}</p>
      {actions.length > 0 && <div className="blue-companion-panel__actions" aria-label="学习行动">{actions.map(action => <button key={action.id} type="button" disabled={action.disabled} onClick={() => {closePanel(); action.onSelect();}}><span>{action.label}</span><ArrowUpRight size={16} aria-hidden="true"/></button>)}</div>}
      <section className="blue-companion-panel__focus" aria-label="10 分钟专注计时">
        <div className="blue-companion-panel__focus-heading"><Clock3 size={15} aria-hidden="true"/><span>专注一小会儿</span><span className="blue-companion-panel__local">仅本地</span></div>
        <div className="blue-companion-panel__timer-row"><span className="blue-companion-panel__time" role="timer" aria-label={`剩余 ${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`} aria-live="off">{time}</span><span className="blue-companion-panel__timer-label">{timerLabel}</span></div>
        <div className="blue-companion-panel__timer-controls">
          {timerState === 'running' ? <button type="button" className="blue-companion-panel__timer-primary" onClick={pauseTimer}><Pause size={15} aria-hidden="true"/>暂停</button> : <button type="button" className="blue-companion-panel__timer-primary" onClick={startTimer}><Play size={15} aria-hidden="true"/>{timerState === 'paused' ? '继续专注' : timerState === 'finished' ? '再来 10 分钟' : '开始 10 分钟'}</button>}
          <button type="button" className="blue-companion-panel__reset" aria-label="重置专注计时" disabled={timerState === 'idle'} onClick={resetTimer}><RotateCcw size={15} aria-hidden="true"/><span>重置</span></button>
        </div>
        <p className="blue-companion-panel__timer-note">收起面板继续计时，离开页面会重置。</p>
      </section>
    </section>, document.body)}
  </div>;
}
