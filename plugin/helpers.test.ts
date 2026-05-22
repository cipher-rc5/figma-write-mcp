import { describe, expect, test } from 'bun:test';

import {
  hexToRgb,
  isFiniteNumber,
  isPositiveNumber,
  isUnitNumber,
  isValidHex,
  KNOWN_OPS,
  parseBridgeMessage,
  parseReq
} from './helpers';

describe('isValidHex', () => {
  test('accepts 6-char hex with or without leading #', () => {
    expect(isValidHex('#111111')).toBe(true);
    expect(isValidHex('111111')).toBe(true);
    expect(isValidHex('#aaBBcc')).toBe(true);
  });
  test('accepts 3-char shorthand', () => {
    expect(isValidHex('#abc')).toBe(true);
    expect(isValidHex('abc')).toBe(true);
  });
  test('rejects garbage', () => {
    expect(isValidHex('not-a-hex')).toBe(false);
    expect(isValidHex('#12345')).toBe(false);
    expect(isValidHex('#12345g')).toBe(false);
    expect(isValidHex('')).toBe(false);
    expect(isValidHex(undefined)).toBe(false);
    expect(isValidHex(0xff0000)).toBe(false);
  });
});

describe('hexToRgb', () => {
  test('parses #111111', () => {
    const rgb = hexToRgb('#111111');
    expect(rgb).not.toBeNull();
    expect(rgb!.r).toBeCloseTo(17 / 255);
    expect(rgb!.g).toBeCloseTo(17 / 255);
    expect(rgb!.b).toBeCloseTo(17 / 255);
  });
  test('expands 3-char shorthand', () => {
    const rgb = hexToRgb('abc');
    expect(rgb).not.toBeNull();
    expect(rgb!.r).toBeCloseTo(0xaa / 255);
    expect(rgb!.g).toBeCloseTo(0xbb / 255);
    expect(rgb!.b).toBeCloseTo(0xcc / 255);
  });
  test('returns null on invalid input', () => {
    expect(hexToRgb('garbage')).toBeNull();
    expect(hexToRgb('')).toBeNull();
    expect(hexToRgb('#12345')).toBeNull();
  });
});

describe('isFiniteNumber / isPositiveNumber / isUnitNumber', () => {
  test('finite', () => {
    expect(isFiniteNumber(0)).toBe(true);
    expect(isFiniteNumber(-1.5)).toBe(true);
    expect(isFiniteNumber(Number.NaN)).toBe(false);
    expect(isFiniteNumber(Number.POSITIVE_INFINITY)).toBe(false);
    expect(isFiniteNumber('1' as unknown)).toBe(false);
  });
  test('positive', () => {
    expect(isPositiveNumber(1)).toBe(true);
    expect(isPositiveNumber(0)).toBe(false);
    expect(isPositiveNumber(-1)).toBe(false);
  });
  test('unit', () => {
    expect(isUnitNumber(0)).toBe(true);
    expect(isUnitNumber(0.5)).toBe(true);
    expect(isUnitNumber(1)).toBe(true);
    expect(isUnitNumber(1.0001)).toBe(false);
    expect(isUnitNumber(-0.0001)).toBe(false);
  });
});

describe('parseReq', () => {
  test('happy path', () => {
    const req = parseReq({ id: 'x', op: 'set_text', params: { node_id: 'a', text: 'b' } });
    expect(req).not.toBeNull();
    expect(req!.id).toBe('x');
    expect(req!.op).toBe('set_text');
    expect(req!.params).toEqual({ node_id: 'a', text: 'b' });
  });
  test('omitted params is fine', () => {
    const req = parseReq({ id: 'x', op: 'ping' });
    expect(req).not.toBeNull();
    expect(req!.params).toBeUndefined();
  });
  test('preserves protocol_version', () => {
    const req = parseReq({ id: 'x', op: 'ping', protocol_version: 1 });
    expect(req!.protocol_version).toBe(1);
  });
  test('rejects unknown op', () => {
    expect(parseReq({ id: 'x', op: 'eject', params: {} })).toBeNull();
  });
  test('rejects missing id', () => {
    expect(parseReq({ op: 'ping' })).toBeNull();
  });
  test('rejects non-object', () => {
    expect(parseReq(null)).toBeNull();
    expect(parseReq('hello')).toBeNull();
    expect(parseReq(42)).toBeNull();
  });
  test('rejects non-object params', () => {
    expect(parseReq({ id: 'x', op: 'set_text', params: 'no' })).toBeNull();
  });
  test('rejects non-number protocol_version', () => {
    expect(parseReq({ id: 'x', op: 'ping', protocol_version: 'one' })).toBeNull();
  });
});

describe('parseBridgeMessage', () => {
  test('accepts well-formed bridge message', () => {
    const msg = parseBridgeMessage({ kind: 'from-bridge', payload: '{}' });
    expect(msg).not.toBeNull();
    expect(msg!.payload).toBe('{}');
  });
  test('rejects wrong kind', () => {
    expect(parseBridgeMessage({ kind: 'other', payload: '{}' })).toBeNull();
  });
  test('rejects non-string payload', () => {
    expect(parseBridgeMessage({ kind: 'from-bridge', payload: 42 })).toBeNull();
  });
  test('rejects null', () => {
    expect(parseBridgeMessage(null)).toBeNull();
  });
});

describe('KNOWN_OPS', () => {
  test('matches the documented operation set', () => {
    const got = [...KNOWN_OPS].sort();
    const want = ['create_text_node', 'delete_node', 'ping', 'set_text', 'update_node_properties'];
    expect(got).toEqual(want as typeof got);
  });
});

describe('parseReq fuzz', () => {
  test('never throws on random object input', () => {
    // Property-style: deterministic small fuzz to assert that parseReq never
    // throws on arbitrary shaped input.
    const cases: unknown[] = [
      null,
      undefined,
      0,
      '',
      [],
      {},
      { id: 1 },
      { id: 'x', op: null },
      { id: 'x', op: 'set_text', params: null },
      { id: 'x', op: 'set_text', params: [] },
      { id: 'x', op: 'set_text', params: { x: { nested: { y: 1 } } } },
      { id: 'x', op: 'ping', protocol_version: { obj: true } }
    ];
    for (const c of cases) {
      expect(() => parseReq(c)).not.toThrow();
    }
  });
});
