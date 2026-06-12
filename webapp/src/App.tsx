import React from 'react';
import { Badge } from './components/Badge';
import { fa } from './fa';
import { engine } from './engine/engine';
import { useEngine } from './engine/useEngine';
import { LoaderScreen } from './screens/Loader';
import { ModeScreen } from './screens/Mode';
import { LiveScreen } from './screens/Live';
import { MediaScreen } from './screens/Media';

export type Stage = 'loader' | 'mode' | 'live' | 'media';

const STAGE_KEY = 've-web-stage';

function readStage(): Stage {
  const raw = localStorage.getItem(STAGE_KEY);
  return raw === 'mode' || raw === 'live' || raw === 'media' ? raw : 'loader';
}

function WebHeader({ onHome }: { onHome: () => void }) {
  const state = useEngine();
  const ready = state.status === 'ready';
  return (
    <header
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 14,
        padding: '14px clamp(16px,3vw,36px)',
        borderBottom: '1px solid var(--border-subtle)',
        background: 'var(--surface-card)',
      }}
    >
      <div
        onClick={ready ? onHome : undefined}
        style={{ display: 'flex', alignItems: 'center', gap: 12, cursor: ready ? 'pointer' : 'default' }}
      >
        <img
          src="/assets/visualears-logo.png"
          alt=""
          style={{ width: 40, height: 40, borderRadius: 10, objectFit: 'cover' }}
        />
        <span style={{ fontFamily: 'var(--font-display)', fontSize: 24, color: 'var(--text-strong)' }}>
          {fa.header.wordmarkA}
          <span style={{ color: 'var(--gold-600)' }}>{fa.header.wordmarkB}</span>
        </span>
        <span
          style={{
            font: 'var(--type-caption)',
            color: 'var(--text-faint)',
            alignSelf: 'flex-end',
            paddingBottom: 3,
          }}
        >
          {fa.header.domain}
        </span>
      </div>
      <div style={{ marginInlineStart: 'auto', display: 'flex', gap: 8, alignItems: 'center' }}>
        <Badge tone={ready ? 'positive' : 'warning'} dot>
          {ready ? fa.header.modelReady : fa.header.modelLoading}
        </Badge>
        <Badge tone="neutral">{engine.providerLabel()}</Badge>
      </div>
    </header>
  );
}

export function App() {
  const [stage, setStage] = React.useState<Stage>(readStage);
  const go = React.useCallback((s: Stage) => {
    setStage(s);
    localStorage.setItem(STAGE_KEY, s);
  }, []);

  // Boot: the loader only shows when the model isn't cached yet. With a cached
  // model the engine loads silently and we land on the mode picker directly.
  React.useEffect(() => {
    let cancelled = false;
    void engine.probeCaps();
    void engine.isModelCached().then((cached) => {
      if (cancelled) return;
      if (cached) {
        void engine.ensureLoaded().catch(() => undefined);
        setStage((s) => (s === 'loader' ? 'mode' : s));
      } else {
        setStage('loader');
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <>
      <WebHeader onHome={() => go('mode')} />
      {stage === 'loader' && <LoaderScreen onReady={() => go('mode')} />}
      {stage === 'mode' && <ModeScreen onPick={go} />}
      {stage === 'live' && <LiveScreen onBack={() => go('mode')} />}
      {stage === 'media' && <MediaScreen onBack={() => go('mode')} />}
    </>
  );
}
