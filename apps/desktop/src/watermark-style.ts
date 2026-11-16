import { bridge, native, type WatermarkOptions, type WatermarkStyle } from './bridge';

export const defaultWatermarkOptions: WatermarkOptions = {
  city: '',
  fallbackDate: null,
  font: 'sans',
  sizePercent: 2,
  color: '#ffffff',
  opacity: 0.88,
  marginPercent: 2.5,
  shadow: true,
  outputMaxEdge: null,
  jpegQuality: 92,
};
export function styleFrom(options: WatermarkOptions): WatermarkStyle {
  const { font, sizePercent, color, opacity, marginPercent, shadow } = options;
  return { font, sizePercent, color, opacity, marginPercent, shadow };
}

// This queue outlives a dialog: closing/reopening cannot race an older write
// against the next read. Superseded slider edits coalesce before reaching IPC.
let tail: Promise<void> = Promise.resolve();
let revision = 0;
let demoStyle: WatermarkStyle | null = null;
let pending: WatermarkStyle | null = null;
export async function loadWatermarkStyle(): Promise<WatermarkStyle | null> {
  await tail.catch(() => {});
  return native ? bridge.loadWatermarkStyle() : demoStyle;
}
export function saveWatermarkStyle(style: WatermarkStyle): Promise<void> {
  const ticket = ++revision;
  const value = { ...style };
  pending = value;
  const write = tail
    .catch(() => {})
    .then(async () => {
      if (ticket !== revision) return;
      await new Promise((resolve) => setTimeout(resolve, 200));
      if (ticket !== revision) return;
      if (native) await bridge.saveWatermarkStyle(value);
      else demoStyle = value; // Browser demo only; never shares native preferences.
      if (ticket === revision) pending = null;
    });
  tail = write;
  return write;
}

// Normal native close/quit calls this before acknowledging the exit request.
// A failed write remains pending, including after its dialog was closed.
export async function flushWatermarkStyle(retry = false): Promise<void> {
  if (retry && pending) void saveWatermarkStyle(pending).catch(() => {});
  for (;;) {
    const observed = tail;
    try {
      await observed;
    } catch (error) {
      if (observed === tail) throw error;
    }
    if (observed === tail) return;
  }
}
