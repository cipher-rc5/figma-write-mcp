// Figma Write MCP Bridge - sandbox code.
//
// Receives JSON request envelopes from the UI iframe (which holds the
// WebSocket to the Rust MCP server), executes them against the Figma
// plugin API, and posts JSON response envelopes back.

import {
  type AppliedProps,
  type IgnoredProps,
  type Req,
  type Resp,
  errorMessage,
  hexToRgb,
  isFiniteNumber,
  isPositiveNumber,
  isUnitNumber,
  isValidHex,
  parseBridgeMessage,
  parseReq
} from './helpers';

const UI_WIDTH = 360;
const UI_HEIGHT = 380;
const SECRET_KEY = 'bridge_secret_v1';

figma.showUI(__html__, { width: UI_WIDTH, height: UI_HEIGHT, themeColors: true });

// Type guards co-located with their type imports for readability.
function isChildrenContainer(node: BaseNode): node is BaseNode & ChildrenMixin {
  return 'appendChild' in node;
}
function isLayoutMixin(node: BaseNode): node is BaseNode & LayoutMixin {
  return 'resize' in node;
}
function isBlendMixin(node: BaseNode): node is BaseNode & BlendMixin {
  return 'opacity' in node;
}
function isSceneNode(node: BaseNode): node is SceneNode {
  return 'visible' in node;
}

function send(resp: Resp): void {
  figma.ui.postMessage({ kind: 'to-bridge', payload: JSON.stringify(resp) });
}

function err(id: string, code: string, message: string): Resp {
  return { id, ok: false, error: { code, message } };
}

async function loadFontsFor(node: TextNode): Promise<void> {
  if (node.characters.length === 0) return;
  const fonts = node.getRangeAllFontNames(0, node.characters.length);
  await Promise.all(fonts.map(figma.loadFontAsync));
}

async function handleSetText(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const node_id = p['node_id'];
  const text = p['text'];
  if (typeof node_id !== 'string' || typeof text !== 'string') {
    return err(req.id, 'invalid_params', 'node_id and text are required strings');
  }
  const node = await figma.getNodeByIdAsync(node_id);
  if (!node) return err(req.id, 'node_not_found', `no node with id ${node_id}`);
  if (node.type !== 'TEXT') return err(req.id, 'wrong_node_type', `node ${node_id} is ${node.type}, not TEXT`);
  const tn: TextNode = node;
  try {
    await loadFontsFor(tn);
  } catch (e: unknown) {
    return err(req.id, 'font_not_loaded', errorMessage(e));
  }
  const previous_text = tn.characters;
  tn.characters = text;
  return { id: req.id, ok: true, result: { node_id, previous_text } };
}

async function handleDeleteNode(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const node_id = p['node_id'];
  if (typeof node_id !== 'string') return err(req.id, 'invalid_params', 'node_id is required');
  const node = await figma.getNodeByIdAsync(node_id);
  if (!node) return err(req.id, 'node_not_found', `no node with id ${node_id}`);
  const parent_id = node.parent ? node.parent.id : '';
  node.remove();
  return { id: req.id, ok: true, result: { node_id, parent_id } };
}

async function handleCreateTextNode(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const parent_id = p['parent_id'];
  const text = p['text'];
  if (typeof parent_id !== 'string' || typeof text !== 'string') {
    return err(req.id, 'invalid_params', 'parent_id and text are required strings');
  }

  const familyRaw = p['font_family'];
  const styleRaw = p['font_style'];
  if (familyRaw !== undefined && typeof familyRaw !== 'string') {
    return err(req.id, 'invalid_params', 'font_family must be a string');
  }
  if (styleRaw !== undefined && typeof styleRaw !== 'string') {
    return err(req.id, 'invalid_params', 'font_style must be a string');
  }

  const fontSize = p['font_size'];
  if (fontSize !== undefined && !isPositiveNumber(fontSize)) {
    return err(req.id, 'invalid_params', 'font_size must be a positive finite number');
  }
  const lineHeightPct = p['line_height_pct'];
  if (lineHeightPct !== undefined && !isPositiveNumber(lineHeightPct)) {
    return err(req.id, 'invalid_params', 'line_height_pct must be a positive finite number');
  }
  const width = p['width'];
  if (width !== undefined && !isPositiveNumber(width)) {
    return err(req.id, 'invalid_params', 'width must be a positive finite number');
  }
  const x = p['x'];
  if (x !== undefined && !isFiniteNumber(x)) {
    return err(req.id, 'invalid_params', 'x must be a finite number');
  }
  const y = p['y'];
  if (y !== undefined && !isFiniteNumber(y)) {
    return err(req.id, 'invalid_params', 'y must be a finite number');
  }
  const fillHex = p['fill_hex'];
  if (fillHex !== undefined && !isValidHex(fillHex)) {
    return err(req.id, 'invalid_params', 'fill_hex must match #rgb or #rrggbb');
  }

  const parentNode = await figma.getNodeByIdAsync(parent_id);
  if (!parentNode) return err(req.id, 'node_not_found', `no parent with id ${parent_id}`);
  if (!isChildrenContainer(parentNode)) {
    return err(req.id, 'wrong_node_type', `parent ${parent_id} cannot accept children`);
  }

  const family = typeof familyRaw === 'string' ? familyRaw : 'Inter';
  const style = typeof styleRaw === 'string' ? styleRaw : 'Regular';
  try {
    await figma.loadFontAsync({ family, style });
  } catch (e: unknown) {
    return err(req.id, 'font_not_loaded', errorMessage(e));
  }

  const tn = figma.createText();
  tn.fontName = { family, style };
  if (isPositiveNumber(fontSize)) tn.fontSize = fontSize;
  tn.characters = text;

  if (isPositiveNumber(lineHeightPct)) {
    tn.lineHeight = { value: lineHeightPct, unit: 'PERCENT' };
  }
  if (isValidHex(fillHex)) {
    const rgb = hexToRgb(fillHex);
    if (rgb) tn.fills = [{ type: 'SOLID', color: rgb }];
  }
  if (isPositiveNumber(width)) {
    tn.textAutoResize = 'HEIGHT';
    tn.resize(width, tn.height);
  }
  if (isFiniteNumber(x)) tn.x = x;
  if (isFiniteNumber(y)) tn.y = y;

  parentNode.appendChild(tn);
  return { id: req.id, ok: true, result: { node_id: tn.id } };
}

async function handleUpdateNodeProperties(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const node_id = p['node_id'];
  const setRaw = p['set'];
  if (typeof node_id !== 'string' || typeof setRaw !== 'object' || setRaw === null) {
    return err(req.id, 'invalid_params', 'node_id and set object are required');
  }
  const set = setRaw as Record<string, unknown>;

  const sx = set['x'];
  const sy = set['y'];
  const sw = set['width'];
  const sh = set['height'];
  const sr = set['rotation'];
  const so = set['opacity'];
  const sv = set['visible'];
  const sn = set['name'];

  if (sx !== undefined && !isFiniteNumber(sx)) return err(req.id, 'invalid_params', 'x must be a finite number');
  if (sy !== undefined && !isFiniteNumber(sy)) return err(req.id, 'invalid_params', 'y must be a finite number');
  if (sw !== undefined && !isPositiveNumber(sw)) {
    return err(req.id, 'invalid_params', 'width must be a positive finite number');
  }
  if (sh !== undefined && !isPositiveNumber(sh)) {
    return err(req.id, 'invalid_params', 'height must be a positive finite number');
  }
  if (sr !== undefined && !isFiniteNumber(sr)) {
    return err(req.id, 'invalid_params', 'rotation must be a finite number');
  }
  if (so !== undefined && !isUnitNumber(so)) {
    return err(req.id, 'invalid_params', 'opacity must be a number in [0, 1]');
  }
  if (sv !== undefined && typeof sv !== 'boolean') {
    return err(req.id, 'invalid_params', 'visible must be a boolean');
  }
  if (sn !== undefined && typeof sn !== 'string') {
    return err(req.id, 'invalid_params', 'name must be a string');
  }

  const node = await figma.getNodeByIdAsync(node_id);
  if (!node) return err(req.id, 'node_not_found', `no node with id ${node_id}`);

  const applied: AppliedProps = {};
  const ignored: IgnoredProps = {};

  try {
    if (isFiniteNumber(sx)) {
      if (isLayoutMixin(node)) {
        node.x = sx;
        applied.x = sx;
      } else ignored['x'] = 'wrong_node_type';
    }
    if (isFiniteNumber(sy)) {
      if (isLayoutMixin(node)) {
        node.y = sy;
        applied.y = sy;
      } else ignored['y'] = 'wrong_node_type';
    }
    if (isPositiveNumber(sw) || isPositiveNumber(sh)) {
      if (isLayoutMixin(node)) {
        const w = isPositiveNumber(sw) ? sw : node.width;
        const h = isPositiveNumber(sh) ? sh : node.height;
        node.resize(w, h);
        if (isPositiveNumber(sw)) applied.width = sw;
        if (isPositiveNumber(sh)) applied.height = sh;
      } else {
        if (isPositiveNumber(sw)) ignored['width'] = 'wrong_node_type';
        if (isPositiveNumber(sh)) ignored['height'] = 'wrong_node_type';
      }
    }
    if (isFiniteNumber(sr)) {
      if (isLayoutMixin(node)) {
        node.rotation = sr;
        applied.rotation = sr;
      } else ignored['rotation'] = 'wrong_node_type';
    }
    if (isUnitNumber(so)) {
      if (isBlendMixin(node)) {
        node.opacity = so;
        applied.opacity = so;
      } else ignored['opacity'] = 'wrong_node_type';
    }
    if (typeof sv === 'boolean') {
      if (isSceneNode(node)) {
        node.visible = sv;
        applied.visible = sv;
      } else ignored['visible'] = 'wrong_node_type';
    }
    if (typeof sn === 'string') {
      node.name = sn;
      applied.name = sn;
    }
  } catch (e: unknown) {
    return err(req.id, 'internal', errorMessage(e));
  }
  return { id: req.id, ok: true, result: { node_id, applied, ignored } };
}

function assertNever(x: never): never {
  throw new Error(`unreachable op: ${String(x)}`);
}

async function dispatch(req: Req): Promise<Resp> {
  try {
    switch (req.op) {
      case 'set_text':
        return await handleSetText(req);
      case 'delete_node':
        return await handleDeleteNode(req);
      case 'create_text_node':
        return await handleCreateTextNode(req);
      case 'update_node_properties':
        return await handleUpdateNodeProperties(req);
      case 'ping':
        return { id: req.id, ok: true, result: { pong: true } };
      default:
        return assertNever(req.op);
    }
  } catch (e: unknown) {
    return err(req.id, 'internal', errorMessage(e));
  }
}

// -----------------------------------------------------------------------------
// Secret management. The UI iframe holds the live WebSocket and performs the
// hello handshake, so the sandbox is the persistent place to remember the
// secret across plugin restarts via figma.clientStorage.
// -----------------------------------------------------------------------------

async function getStoredSecret(): Promise<string | null> {
  const s: unknown = await figma.clientStorage.getAsync(SECRET_KEY);
  return typeof s === 'string' && s.length > 0 ? s : null;
}

async function setStoredSecret(secret: string | null): Promise<void> {
  if (secret === null || secret.length === 0) {
    await figma.clientStorage.deleteAsync(SECRET_KEY);
    return;
  }
  await figma.clientStorage.setAsync(SECRET_KEY, secret);
}

async function sendSecretToUi(): Promise<void> {
  const secret = await getStoredSecret();
  figma.ui.postMessage({ kind: 'set-secret', secret });
}

type SaveSecretMessage = { kind: 'save-secret'; secret: string };
type ClearSecretMessage = { kind: 'clear-secret' };
type UiReadyMessage = { kind: 'ui-ready' };
type LogMessage = { kind: 'log'; line: string };
type IncomingUiMessage =
  | SaveSecretMessage
  | ClearSecretMessage
  | UiReadyMessage
  | LogMessage
  | { kind: 'from-bridge'; payload: string };

function parseUiMessage(raw: unknown): IncomingUiMessage | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const obj = raw as Record<string, unknown>;
  const kind = obj['kind'];
  switch (kind) {
    case 'ui-ready':
      return { kind: 'ui-ready' };
    case 'clear-secret':
      return { kind: 'clear-secret' };
    case 'save-secret': {
      const secret = obj['secret'];
      if (typeof secret !== 'string' || secret.length === 0) return null;
      return { kind: 'save-secret', secret };
    }
    case 'log': {
      const line = obj['line'];
      if (typeof line !== 'string') return null;
      return { kind: 'log', line };
    }
    case 'from-bridge': {
      const bridge = parseBridgeMessage(obj);
      return bridge;
    }
    default:
      return null;
  }
}

figma.ui.onmessage = async (raw: unknown) => {
  const msg = parseUiMessage(raw);
  if (!msg) return;
  switch (msg.kind) {
    case 'ui-ready':
      await sendSecretToUi();
      return;
    case 'save-secret':
      await setStoredSecret(msg.secret);
      await sendSecretToUi();
      return;
    case 'clear-secret':
      await setStoredSecret(null);
      await sendSecretToUi();
      return;
    case 'log':
      console.log(`[ui] ${msg.line}`);
      return;
    case 'from-bridge': {
      let parsed: unknown;
      try {
        parsed = JSON.parse(msg.payload);
      } catch (e) {
        // No id to echo - log via the UI panel rather than dropping silently.
        figma.ui.postMessage({
          kind: 'log',
          line: `dropped malformed bridge frame: ${errorMessage(e)}`
        });
        return;
      }
      const req = parseReq(parsed);
      if (!req) {
        const maybeId =
          typeof parsed === 'object' && parsed !== null ? (parsed as Record<string, unknown>)['id'] : undefined;
        if (typeof maybeId === 'string') {
          send(err(maybeId, 'invalid_params', 'malformed request envelope'));
        } else {
          figma.ui.postMessage({
            kind: 'log',
            line: 'dropped malformed bridge frame: no id to echo'
          });
        }
        return;
      }
      const resp = await dispatch(req);
      send(resp);
      return;
    }
  }
};
