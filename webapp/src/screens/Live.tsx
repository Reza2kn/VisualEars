import React from 'react';
import { ArrowRight, Mic, MonitorSpeaker, Pause, Play } from 'lucide-react';
import { Button } from '../components/Button';
import { IconButton } from '../components/IconButton';
import { WaveBars } from '../components/WaveBars';
import { fa } from '../fa';
import { faDigits, faTimestamp } from '../format';
import { LiveSession, type LiveSnapshot, type LiveSource, type Utterance } from '../engine/liveSession';
import { useEngine } from '../engine/useEngine';

export const SPEAKER_COLORS = [
  'var(--terracotta)',
  'var(--olive)',
  'var(--sage)',
  'var(--clay)',
  'var(--pomegranate)',
];

function useLiveSession(source: LiveSource): { snapshot: LiveSnapshot; session: LiveSession } {
  const [, force] = React.useReducer((n: number) => n + 1, 0);
  const sessionRef = React.useRef<LiveSession | null>(null);
  if (!sessionRef.current) sessionRef.current = new LiveSession(force);
  const session = sessionRef.current;

  React.useEffect(() => {
    void session.start(source);
    return () => {
      void session.stop();
    };
  }, [session, source]);

  return { snapshot: session.getSnapshot(), session };
}

function TranscriptLine({
  line,
  elevated,
  caret,
}: {
  line: Utterance;
  elevated: boolean;
  caret: boolean;
}) {
  return (
    <div style={{ display: 'flex', gap: 12, alignItems: 'flex-start' }}>
      <span
        className="ve-fa-digits"
        style={{ font: 'var(--type-caption)', color: 'var(--text-faint)', minWidth: 42, paddingTop: 6 }}
      >
        {faTimestamp(line.tStart)}
      </span>
      <div style={{ flex: 1 }}>
        <div
          style={{
            font: 'var(--type-label)',
            color: SPEAKER_COLORS[line.speakerId % SPEAKER_COLORS.length],
            marginBottom: 2,
          }}
        >
          {fa.live.speaker(faDigits(line.speakerId + 1))}
        </div>
        <div
          style={{
            font: elevated ? `400 var(--text-xl)/1.45 var(--font-display)` : 'var(--type-body)',
            color: 'var(--text-strong)',
            background: elevated ? 'var(--surface-card)' : 'transparent',
            border: elevated ? '1px solid var(--border-subtle)' : 'none',
            borderRadius: 'var(--radius-md)',
            padding: elevated ? '10px 16px' : 0,
            boxShadow: elevated ? 'var(--shadow-sm)' : 'none',
          }}
        >
          {line.text}
          {caret && <span className="ve-caret" />}
        </div>
      </div>
    </div>
  );
}

export function LiveScreen({ onBack }: { onBack: () => void }) {
  const [source, setSource] = React.useState<LiveSource>('mic');
  const { snapshot, session } = useLiveSession(source);
  const engineState = useEngine();
  const scrollRef = React.useRef<HTMLDivElement | null>(null);

  const lines: Utterance[] = snapshot.partial
    ? [...snapshot.utterances, snapshot.partial]
    : snapshot.utterances;

  React.useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
    if (nearBottom) el.scrollTop = el.scrollHeight;
  }, [lines.length, snapshot.partial?.text]);

  const stats = engineState.lastStats;
  const providerStat =
    engineState.provider === 'webgpu' ? 'WebGPU' : `${engineState.threads} threads`;

  return (
    <div dir="rtl" lang="fa" style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          padding: '14px clamp(16px,3vw,36px)',
          flexWrap: 'wrap',
        }}
      >
        <IconButton label={fa.live.back} variant="soft" size="sm" onClick={onBack}>
          <ArrowRight size={18} />
        </IconButton>
        <h2 style={{ fontSize: 'var(--text-xl)', margin: 0 }}>{fa.live.title}</h2>
        <div style={{ display: 'flex', gap: 6, marginInlineStart: 'auto' }}>
          <Button
            variant={source === 'mic' ? 'primary' : 'secondary'}
            size="sm"
            iconStart={<Mic size={16} />}
            onClick={() => setSource('mic')}
          >
            {fa.live.sourceMic}
          </Button>
          <Button
            variant={source === 'sys' ? 'primary' : 'secondary'}
            size="sm"
            iconStart={<MonitorSpeaker size={16} />}
            onClick={() => setSource('sys')}
          >
            {fa.live.sourceSystem}
          </Button>
        </div>
      </div>

      <div
        style={{
          margin: '0 clamp(16px,3vw,36px)',
          borderRadius: 'var(--radius-lg)',
          background: 'var(--grad-ember)',
          padding: '14px 20px',
          display: 'flex',
          alignItems: 'center',
          gap: 18,
          flexWrap: 'wrap',
        }}
      >
        <WaveBars
          active={snapshot.running}
          count={26}
          height={44}
          color="var(--gold-300)"
          levels={snapshot.running ? snapshot.levels : undefined}
          style={{ flex: 1, minWidth: 200 }}
        />
        <div
          style={{
            display: 'flex',
            gap: 14,
            font: 'var(--type-caption)',
            color: 'var(--cream-300)',
            direction: 'ltr',
          }}
        >
          <span>{stats ? `RTF ${stats.rtf.toFixed(2)}×` : 'RTF —'}</span>
          <span>{stats ? `~${Math.round(stats.totalMs)} ms` : '~— ms'}</span>
          <span>{providerStat}</span>
        </div>
        <IconButton
          label={snapshot.running ? fa.live.pause : fa.live.resume}
          variant="accent"
          onClick={() => void (snapshot.running ? session.pause() : session.resume())}
        >
          {snapshot.running ? <Pause size={22} /> : <Play size={22} />}
        </IconButton>
      </div>

      <div
        ref={scrollRef}
        style={{
          flex: 1,
          minHeight: 0,
          overflow: 'auto',
          padding: 'clamp(16px,3vw,36px)',
          display: 'flex',
          flexDirection: 'column',
          gap: 12,
        }}
      >
        {snapshot.error && (
          <div style={{ font: 'var(--type-body)', color: 'var(--danger)' }}>{snapshot.error}</div>
        )}
        {!snapshot.error && lines.length === 0 && (
          <div style={{ font: 'var(--type-body)', color: 'var(--text-faint)', textAlign: 'center', marginTop: 24 }}>
            {fa.live.quietEmpty}
          </div>
        )}
        {lines.map((line, i) => {
          const isLast = i === lines.length - 1;
          return (
            <TranscriptLine
              key={i}
              line={line}
              elevated={isLast}
              caret={isLast && snapshot.running}
            />
          );
        })}
      </div>
    </div>
  );
}
