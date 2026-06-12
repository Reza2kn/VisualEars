import { FileAudio, Mic } from 'lucide-react';
import { Card } from '../components/Card';
import { fa } from '../fa';
import type { Stage } from '../App';

export function ModeScreen({ onPick }: { onPick: (stage: Stage) => void }) {
  const modes = [
    {
      id: 'live' as const,
      icon: <Mic size={28} />,
      color: 'var(--crimson-500)',
      title: fa.mode.live.title,
      body: fa.mode.live.body,
    },
    {
      id: 'media' as const,
      icon: <FileAudio size={28} />,
      color: 'var(--terracotta)',
      title: fa.mode.media.title,
      body: fa.mode.media.body,
    },
  ];
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
        gap: 28,
        padding: 'clamp(20px,4vw,48px)',
      }}
    >
      <h1 style={{ fontSize: 'clamp(30px,4vw,44px)' }}>{fa.mode.title}</h1>
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit,minmax(260px,1fr))',
          gap: 20,
          width: 'min(100%, 760px)',
        }}
      >
        {modes.map((m) => (
          <Card key={m.id} tone="raised" interactive onClick={() => onPick(m.id)} style={{ textAlign: 'start' }}>
            <span
              style={{
                width: 60,
                height: 60,
                borderRadius: 'var(--radius-lg)',
                background: m.color,
                color: '#fff',
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                marginBottom: 14,
              }}
            >
              {m.icon}
            </span>
            <h3 style={{ fontSize: 'var(--text-xl)' }}>{m.title}</h3>
            <p style={{ font: 'var(--type-body)', color: 'var(--text-muted)', margin: '8px 0 0' }}>{m.body}</p>
          </Card>
        ))}
      </div>
    </div>
  );
}
