import React from 'react';
import { CircleCheck, CircleMinus, Download } from 'lucide-react';
import { Badge } from '../components/Badge';
import { Button } from '../components/Button';
import { Card } from '../components/Card';
import { fa } from '../fa';
import { faDigits, faPercent } from '../format';
import { MODEL_VARIANTS } from '../engine/manifest';
import { engine } from '../engine/engine';
import { useEngine } from '../engine/useEngine';

export function LoaderScreen({ onReady }: { onReady: () => void }) {
  const state = useEngine();
  const [variantId, setVariantId] = React.useState(state.variant.id);
  const started = state.status !== 'idle' && state.status !== 'error';

  React.useEffect(() => {
    if (state.status === 'ready') {
      const id = setTimeout(onReady, 350);
      return () => clearTimeout(id);
    }
  }, [state.status, onReady]);

  const caps = state.caps;
  const capRows = [
    { key: fa.loader.capSimd, ok: caps?.simd ?? false, note: '' },
    {
      key: fa.loader.capThreads(faDigits(caps?.maxThreads ?? 1)),
      ok: caps?.threads ?? false,
      note: '',
    },
    {
      key: fa.loader.capWebGpu,
      ok: caps?.webgpu ?? false,
      note: caps?.webgpu ? '' : fa.loader.capWebGpuNotNeeded,
    },
  ];

  const pct =
    state.status === 'ready'
      ? 100
      : state.status === 'initializing'
        ? 96
        : Math.min(92, (state.loadedBytes / Math.max(1, state.totalBytes)) * 92);

  const statusText =
    state.status === 'ready'
      ? fa.loader.ready
      : state.status === 'initializing'
        ? caps?.webgpu
          ? fa.loader.initWebGpu
          : fa.loader.initWasm
        : fa.loader.downloading;

  return (
    <div
      dir="rtl"
      lang="fa"
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 26,
        padding: 'clamp(20px,4vw,48px)',
        textAlign: 'center',
        overflow: 'auto',
      }}
    >
      <img
        src="/assets/visualears-logo.png"
        alt="VisualEars"
        style={{
          width: 130,
          height: 130,
          borderRadius: 'var(--radius-xl)',
          boxShadow: 'var(--shadow-lg)',
          objectFit: 'cover',
        }}
      />
      <div>
        <h1 style={{ fontSize: 'clamp(34px,5vw,52px)' }}>{fa.loader.title}</h1>
        <p
          style={{
            font: 'var(--type-lead)',
            color: 'var(--text-muted)',
            margin: '8px auto 0',
            maxWidth: '42ch',
          }}
        >
          {fa.loader.lead}
        </p>
      </div>

      {!started ? (
        <Card tone="raised" style={{ width: 'min(100%, 480px)', textAlign: 'start' }} padding="var(--space-5)">
          <div style={{ font: 'var(--type-label)', marginBottom: 12 }}>{fa.loader.whichModel}</div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {MODEL_VARIANTS.map((m) => {
              const selected = variantId === m.id;
              return (
                <div
                  key={m.id}
                  onClick={() => setVariantId(m.id)}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 12,
                    padding: '12px 14px',
                    borderRadius: 'var(--radius-md)',
                    cursor: 'pointer',
                    background: selected ? 'var(--crimson-50)' : 'var(--surface-sunken)',
                    border: `1.5px solid ${selected ? 'var(--border-brand)' : 'transparent'}`,
                  }}
                >
                  <span
                    style={{
                      width: 18,
                      height: 18,
                      borderRadius: '50%',
                      border: `2px solid ${selected ? 'var(--crimson-500)' : 'var(--border-default)'}`,
                      display: 'inline-flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                      flex: 'none',
                    }}
                  >
                    {selected && (
                      <span
                        style={{ width: 9, height: 9, borderRadius: '50%', background: 'var(--crimson-500)' }}
                      />
                    )}
                  </span>
                  <div style={{ flex: 1 }}>
                    <div style={{ font: 'var(--type-ui)' }}>
                      {m.label} {m.recommended && <Badge tone="accent">{fa.loader.recommended}</Badge>}
                    </div>
                    <div
                      style={{
                        font: 'var(--type-caption)',
                        color: 'var(--text-muted)',
                        direction: 'ltr',
                        textAlign: 'end',
                      }}
                    >
                      {m.metaLine}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6, margin: '16px 0' }}>
            {capRows.map((c) => (
              <div
                key={c.key}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  font: 'var(--type-caption)',
                  color: 'var(--text-muted)',
                }}
              >
                <span style={{ color: c.ok ? 'var(--positive)' : 'var(--text-faint)', display: 'inline-flex' }}>
                  {c.ok ? <CircleCheck size={16} /> : <CircleMinus size={16} />}
                </span>
                {c.key}
                {c.note ? ` — ${c.note}` : ''}
              </div>
            ))}
          </div>
          {state.status === 'error' && (
            <div
              style={{
                font: 'var(--type-caption)',
                color: 'var(--danger)',
                marginBottom: 12,
              }}
            >
              {fa.loader.loadFailed}
            </div>
          )}
          <Button
            variant="primary"
            fullWidth
            iconStart={<Download size={20} />}
            onClick={() => void engine.ensureLoaded().catch(() => undefined)}
          >
            {state.status === 'error' ? fa.loader.retry : fa.loader.download}
          </Button>
        </Card>
      ) : (
        <Card tone="raised" style={{ width: 'min(100%, 480px)' }} padding="var(--space-5)">
          <div
            style={{ display: 'flex', justifyContent: 'space-between', font: 'var(--type-ui)', marginBottom: 10 }}
          >
            <span>{statusText}</span>
            <span className="ve-fa-digits">{faPercent(pct)}</span>
          </div>
          <div
            style={{
              height: 10,
              borderRadius: 'var(--radius-pill)',
              background: 'var(--cream-300)',
              overflow: 'hidden',
            }}
          >
            <span
              style={{
                display: 'block',
                height: '100%',
                width: `${pct}%`,
                background: 'var(--grad-saffron)',
                borderRadius: 'var(--radius-pill)',
                transition: 'width .2s ease',
              }}
            />
          </div>
          <div
            style={{
              font: 'var(--type-caption)',
              color: 'var(--text-muted)',
              marginTop: 8,
              direction: 'ltr',
              textAlign: 'start',
            }}
          >
            {fa.loader.cachedCaption(state.variant.graph.name)}
          </div>
        </Card>
      )}
      <div style={{ font: 'var(--type-caption)', color: 'var(--text-faint)' }}>{fa.loader.footer}</div>
    </div>
  );
}
