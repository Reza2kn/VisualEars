import React from 'react';
import { ArrowRight, Download, FileUp } from 'lucide-react';
import { Button } from '../components/Button';
import { IconButton } from '../components/IconButton';
import { WaveBars } from '../components/WaveBars';
import { fa } from '../fa';
import { faDigits, faPercent, faTimestamp } from '../format';
import { decodeFileToPcm, transcribePcm, type Segment } from '../engine/mediaSession';
import { downloadText, toSrt, toVtt } from '../engine/srtvtt';
import { useEngine } from '../engine/useEngine';
import { SPEAKER_COLORS } from './Live';

type Phase = 'drop' | 'work' | 'done';
type Kind = 'video' | 'audio';

const VIDEO_EXT = /\.(mp4|mov|mkv|webm|m4v)$/i;

function fileKind(file: File): Kind {
  if (file.type.startsWith('video/')) return 'video';
  if (file.type.startsWith('audio/')) return 'audio';
  return VIDEO_EXT.test(file.name) ? 'video' : 'audio';
}

function exportName(fileName: string, ext: string): string {
  return `${fileName.replace(/\.[^.]+$/, '')}.${ext}`;
}

export function MediaScreen({ onBack }: { onBack: () => void }) {
  const engineState = useEngine();
  const [phase, setPhase] = React.useState<Phase>('drop');
  const [kind, setKind] = React.useState<Kind>('video');
  const [file, setFile] = React.useState<File | null>(null);
  const [objectUrl, setObjectUrl] = React.useState<string | null>(null);
  const [pct, setPct] = React.useState(0);
  const [segments, setSegments] = React.useState<Segment[]>([]);
  const [error, setError] = React.useState<string | null>(null);
  const [playhead, setPlayhead] = React.useState(0);
  const inputRef = React.useRef<HTMLInputElement | null>(null);
  const videoRef = React.useRef<HTMLVideoElement | null>(null);
  const jobRef = React.useRef(0);

  React.useEffect(() => {
    return () => {
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [objectUrl]);

  const startFile = React.useCallback(async (f: File) => {
    const job = ++jobRef.current;
    setError(null);
    setFile(f);
    setKind(fileKind(f));
    setPct(0);
    setSegments([]);
    setPhase('work');
    setObjectUrl((prev) => {
      if (prev) URL.revokeObjectURL(prev);
      return URL.createObjectURL(f);
    });
    try {
      const pcm = await decodeFileToPcm(f);
      if (jobRef.current !== job) return;
      const segs = await transcribePcm(pcm, ({ pct: p }) => {
        if (jobRef.current === job) setPct(p);
      });
      if (jobRef.current !== job) return;
      if (segs.length === 0) {
        setError(fa.media.nothingHeard);
        setPhase('drop');
        return;
      }
      setSegments(segs);
      setPlayhead(0);
      setPhase('done');
    } catch (err) {
      console.warn('[media] failed:', err);
      if (jobRef.current === job) {
        setError(fa.media.decodeFailed);
        setPhase('drop');
      }
    }
  }, []);

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    const f = e.dataTransfer.files?.[0];
    if (f) void startFile(f);
  };

  const activeIndex = segments.findIndex((s) => playhead >= s.tStart && playhead < s.tEnd);
  const overlayText = activeIndex >= 0 ? segments[activeIndex].text : '';

  const stats = engineState.lastStats;
  const providerName = engineState.provider === 'webgpu' ? 'WebGPU' : 'CPU';

  return (
    <div dir="rtl" lang="fa" style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '14px clamp(16px,3vw,36px)' }}>
        <IconButton label={fa.media.back} variant="soft" size="sm" onClick={onBack}>
          <ArrowRight size={18} />
        </IconButton>
        <h2 style={{ fontSize: 'var(--text-xl)', margin: 0 }}>{fa.media.title}</h2>
        {phase === 'done' && (
          <div style={{ display: 'flex', gap: 8, marginInlineStart: 'auto' }}>
            <Button
              variant="secondary"
              size="sm"
              iconStart={<Download size={16} />}
              onClick={() => file && downloadText(exportName(file.name, 'srt'), toSrt(segments))}
            >
              {fa.media.srt}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              iconStart={<Download size={16} />}
              onClick={() => file && downloadText(exportName(file.name, 'vtt'), toVtt(segments))}
            >
              {fa.media.vtt}
            </Button>
          </div>
        )}
      </div>

      {phase === 'drop' && (
        <div
          onDragOver={(e) => e.preventDefault()}
          onDrop={onDrop}
          style={{
            flex: 1,
            margin: '0 clamp(16px,3vw,36px) 24px',
            border: '2px dashed var(--border-brand)',
            borderRadius: 'var(--radius-xl)',
            background: 'var(--crimson-50)',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 14,
            textAlign: 'center',
            padding: 24,
          }}
        >
          <span
            style={{
              width: 76,
              height: 76,
              borderRadius: 'var(--radius-xl)',
              background: 'var(--grad-ember)',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
              color: 'var(--cream-100)',
            }}
          >
            <FileUp size={38} />
          </span>
          <div style={{ fontFamily: 'var(--font-display)', fontSize: 'var(--text-xl)', color: 'var(--text-strong)' }}>
            {fa.media.dropTitle}
          </div>
          <div style={{ font: 'var(--type-caption)', color: 'var(--text-muted)', direction: 'ltr' }}>
            {fa.media.formats}
          </div>
          {error && <div style={{ font: 'var(--type-body)', color: 'var(--danger)' }}>{error}</div>}
          <input
            ref={inputRef}
            type="file"
            accept=".mp4,.mov,.mkv,.mp3,.wav,.m4a,video/*,audio/*"
            style={{ display: 'none' }}
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (f) void startFile(f);
              e.target.value = '';
            }}
          />
          <Button variant="secondary" size="sm" iconStart={<FileUp size={18} />} onClick={() => inputRef.current?.click()}>
            {fa.media.browse}
          </Button>
        </div>
      )}

      {phase === 'work' && (
        <div
          style={{
            flex: 1,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 20,
            padding: 24,
          }}
        >
          <WaveBars active count={22} height={56} color="var(--gold-500)" />
          <div style={{ width: 'min(100%, 420px)' }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', font: 'var(--type-ui)', marginBottom: 8 }}>
              <span>{fa.media.working}</span>
              <span className="ve-fa-digits">{faPercent(pct)}</span>
            </div>
            <div style={{ height: 10, borderRadius: 'var(--radius-pill)', background: 'var(--cream-300)', overflow: 'hidden' }}>
              <span
                style={{
                  display: 'block',
                  height: '100%',
                  width: `${pct}%`,
                  background: 'var(--grad-saffron)',
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
              {file?.name}
              {stats ? ` · RTF ${stats.rtf.toFixed(2)}× on ${providerName}` : ''}
            </div>
          </div>
        </div>
      )}

      {phase === 'done' && (
        <div
          style={{
            flex: 1,
            minHeight: 0,
            display: 'grid',
            gridTemplateColumns: kind === 'video' ? 'minmax(0,1.2fr) minmax(0,1fr)' : '1fr',
            gap: 20,
            padding: '0 clamp(16px,3vw,36px) 24px',
          }}
        >
          {kind === 'video' && objectUrl && (
            <div
              style={{
                alignSelf: 'start',
                borderRadius: 'var(--radius-lg)',
                overflow: 'hidden',
                background: '#111',
                position: 'relative',
                aspectRatio: '16/9',
              }}
            >
              <video
                ref={videoRef}
                src={objectUrl}
                controls
                style={{ width: '100%', height: '100%', objectFit: 'contain' }}
                onTimeUpdate={(e) => setPlayhead(e.currentTarget.currentTime)}
              />
              {overlayText && (
                <div
                  style={{
                    position: 'absolute',
                    bottom: 10,
                    insetInline: 10,
                    background: 'rgba(31,15,8,0.85)',
                    backdropFilter: 'blur(8px)',
                    borderRadius: 'var(--radius-md)',
                    padding: '8px 14px',
                    textAlign: 'center',
                    fontFamily: 'var(--font-display)',
                    fontSize: 'var(--text-lg)',
                    color: 'var(--cream-50)',
                    lineHeight: 1.45,
                    pointerEvents: 'none',
                  }}
                >
                  {overlayText}
                </div>
              )}
            </div>
          )}
          <div style={{ minHeight: 0, overflow: 'auto', display: 'flex', flexDirection: 'column', gap: 4 }}>
            {segments.map((seg, i) => {
              const active = i === activeIndex;
              return (
                <div
                  key={i}
                  onClick={
                    kind === 'video'
                      ? () => {
                          if (videoRef.current) videoRef.current.currentTime = seg.tStart + 0.01;
                        }
                      : undefined
                  }
                  style={{
                    display: 'flex',
                    gap: 10,
                    alignItems: 'flex-start',
                    padding: '8px 12px',
                    borderRadius: 'var(--radius-md)',
                    background: active ? 'var(--crimson-50)' : 'transparent',
                    borderInlineStart: `3px solid ${active ? 'var(--crimson-500)' : 'transparent'}`,
                    cursor: kind === 'video' ? 'pointer' : 'default',
                  }}
                >
                  <span
                    className="ve-fa-digits"
                    style={{ font: 'var(--type-caption)', color: 'var(--text-faint)', minWidth: 40, paddingTop: 4 }}
                  >
                    {faTimestamp(seg.tStart)}
                  </span>
                  <div>
                    <div
                      style={{
                        font: 'var(--type-label)',
                        color: SPEAKER_COLORS[seg.speakerId % SPEAKER_COLORS.length],
                        marginBottom: 2,
                      }}
                    >
                      {fa.live.speaker(faDigits(seg.speakerId + 1))}
                    </div>
                    <div data-testid="segment-text" style={{ font: 'var(--type-body)' }}>
                      {seg.text}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
