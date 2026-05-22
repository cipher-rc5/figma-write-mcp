// Pure helpers shared between the Figma plugin sandbox (code.ts) and the
// unit tests. Nothing in this file may reference `figma`, `__html__`, or
// any other plugin-runtime global, so the tests can import it under
// plain Bun.

export type Op =
  | 'set_text'
  | 'delete_node'
  | 'create_text_node'
  | 'update_node_properties'
  | 'ping';

export type Req = {
  id: string;
  op: Op;
  protocol_version?: number;
  params?: Record<string, unknown>;
};

export type AppliedProps = {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  rotation?: number;
  opacity?: number;
  visible?: boolean;
  name?: string;
};

export type IgnoredProps = Record<string, string>;

export type ResultPayload =
  | { node_id: string; previous_text: string }
  | { node_id: string; parent_id: string }
  | { node_id: string }
  | { node_id: string; applied: AppliedProps; ignored: IgnoredProps }
  | { pong: true };

export type Resp =
  | { id: string; ok: true; result: ResultPayload }
  | { id: string; ok: false; error: { code: string; message: string } };

export type BridgeMessage = { kind: 'from-bridge'; payload: string };

export const PROTOCOL_VERSION = 1 as const;

export const KNOWN_OPS: ReadonlySet<Op> = new Set<Op>([
  'set_text',
  'delete_node',
  'create_text_node',
  'update_node_properties',
  'ping'
]);

export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === 'string') return e;
  return String(e);
}

export function isFiniteNumber(x: unknown): x is number {
  return typeof x === 'number' && Number.isFinite(x);
}

export function isPositiveNumber(x: unknown): x is number {
  return isFiniteNumber(x) && x > 0;
}

export function isUnitNumber(x: unknown): x is number {
  return isFiniteNumber(x) && x >= 0 && x <= 1;
}

export const HEX_RE = /^#?[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/;

export function isValidHex(s: unknown): s is string {
  return typeof s === 'string' && HEX_RE.test(s);
}

export type RGB = { r: number; g: number; b: number };

export function hexToRgb(hex: string): RGB | null {
  if (!isValidHex(hex)) return null;
  const h = hex.replace('#', '');
  const expanded = h.length === 3 ? h.split('').map((c) => c + c).join('') : h;
  const n = parseInt(expanded, 16);
  return {
    r: ((n >> 16) & 255) / 255,
    g: ((n >> 8) & 255) / 255,
    b: (n & 255) / 255
  };
}

export function parseReq(raw: unknown): Req | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const obj = raw as Record<string, unknown>;
  const id = obj['id'];
  const op = obj['op'];
  const params = obj['params'];
  const protocolVersion = obj['protocol_version'];
  if (typeof id !== 'string') return null;
  if (typeof op !== 'string' || !KNOWN_OPS.has(op as Op)) return null;
  if (params !== undefined && (typeof params !== 'object' || params === null)) return null;
  if (protocolVersion !== undefined && typeof protocolVersion !== 'number') return null;
  const base: { id: string; op: Op } = { id, op: op as Op };
  const withParams = params === undefined ? base : { ...base, params: params as Record<string, unknown> };
  const withVersion =
    protocolVersion === undefined ? withParams : { ...withParams, protocol_version: protocolVersion as number };
  return withVersion as Req;
}

export function parseBridgeMessage(raw: unknown): BridgeMessage | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const obj = raw as Record<string, unknown>;
  if (obj['kind'] !== 'from-bridge') return null;
  const payload = obj['payload'];
  if (typeof payload !== 'string') return null;
  return { kind: 'from-bridge', payload };
}
