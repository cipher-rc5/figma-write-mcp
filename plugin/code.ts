// Figma Write MCP Bridge - sandbox code.
//
// Receives JSON request envelopes from the UI iframe (which holds the
// WebSocket to the Rust MCP server), executes them against the Figma
// plugin API, and posts JSON response envelopes back.

const UI_WIDTH = 320;
const UI_HEIGHT = 320;

figma.showUI(__html__, { width: UI_WIDTH, height: UI_HEIGHT, themeColors: true });

type Op =
  | "set_text"
  | "delete_node"
  | "create_text_node"
  | "update_node_properties"
  | "ping";

type Req = {
  id: string;
  op: Op;
  params?: Record<string, unknown>;
};

type AppliedProps = {
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  rotation?: number;
  opacity?: number;
  visible?: boolean;
  name?: string;
};

type ResultPayload =
  | { node_id: string; previous_text: string }
  | { node_id: string; parent_id: string }
  | { node_id: string }
  | { node_id: string; applied: AppliedProps }
  | { pong: true };

type Resp =
  | { id: string; ok: true; result: ResultPayload }
  | { id: string; ok: false; error: { code: string; message: string } };

type BridgeMessage = { kind: "from-bridge"; payload: string };

function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string") return e;
  return String(e);
}

function send(resp: Resp): void {
  figma.ui.postMessage({ kind: "to-bridge", payload: JSON.stringify(resp) });
}

function err(id: string, code: string, message: string): Resp {
  return { id, ok: false, error: { code, message } };
}

async function loadFontsFor(node: TextNode): Promise<void> {
  // Every styled range in a text node can have its own font; load them all
  // before any mutation, or Figma throws.
  const fonts = node.getRangeAllFontNames(0, node.characters.length);
  await Promise.all(fonts.map(figma.loadFontAsync));
}

function hexToRgb(hex: string): RGB {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h, 16);
  return { r: ((n >> 16) & 255) / 255, g: ((n >> 8) & 255) / 255, b: (n & 255) / 255 };
}

async function handleSetText(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const node_id = p["node_id"];
  const text = p["text"];
  if (typeof node_id !== "string" || typeof text !== "string") {
    return err(req.id, "invalid_params", "node_id and text are required strings");
  }
  const node = await figma.getNodeByIdAsync(node_id);
  if (!node) return err(req.id, "node_not_found", `no node with id ${node_id}`);
  if (node.type !== "TEXT") return err(req.id, "wrong_node_type", `node ${node_id} is ${node.type}, not TEXT`);
  const tn: TextNode = node;
  try {
    await loadFontsFor(tn);
  } catch (e: unknown) {
    return err(req.id, "font_not_loaded", errorMessage(e));
  }
  const previous_text = tn.characters;
  tn.characters = text;
  return { id: req.id, ok: true, result: { node_id, previous_text } };
}

async function handleDeleteNode(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const node_id = p["node_id"];
  if (typeof node_id !== "string") return err(req.id, "invalid_params", "node_id is required");
  const node = await figma.getNodeByIdAsync(node_id);
  if (!node) return err(req.id, "node_not_found", `no node with id ${node_id}`);
  const parent_id = node.parent ? node.parent.id : "";
  node.remove();
  return { id: req.id, ok: true, result: { node_id, parent_id } };
}

async function handleCreateTextNode(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const parent_id = p["parent_id"];
  const text = p["text"];
  if (typeof parent_id !== "string" || typeof text !== "string") {
    return err(req.id, "invalid_params", "parent_id and text are required strings");
  }
  const parentNode = await figma.getNodeByIdAsync(parent_id);
  if (!parentNode) return err(req.id, "node_not_found", `no parent with id ${parent_id}`);
  if (!isChildrenContainer(parentNode)) {
    return err(req.id, "wrong_node_type", `parent ${parent_id} cannot accept children`);
  }

  const familyRaw = p["font_family"];
  const styleRaw = p["font_style"];
  const family = typeof familyRaw === "string" ? familyRaw : "Inter";
  const style = typeof styleRaw === "string" ? styleRaw : "Regular";
  try {
    await figma.loadFontAsync({ family, style });
  } catch (e: unknown) {
    return err(req.id, "font_not_loaded", errorMessage(e));
  }

  const tn = figma.createText();
  tn.fontName = { family, style };
  const fontSize = p["font_size"];
  if (typeof fontSize === "number") tn.fontSize = fontSize;
  tn.characters = text;

  const lineHeightPct = p["line_height_pct"];
  if (typeof lineHeightPct === "number") {
    tn.lineHeight = { value: lineHeightPct, unit: "PERCENT" };
  }
  const fillHex = p["fill_hex"];
  if (typeof fillHex === "string") {
    tn.fills = [{ type: "SOLID", color: hexToRgb(fillHex) }];
  }
  const width = p["width"];
  if (typeof width === "number") {
    tn.textAutoResize = "HEIGHT";
    tn.resize(width, tn.height);
  }
  const x = p["x"];
  const y = p["y"];
  if (typeof x === "number") tn.x = x;
  if (typeof y === "number") tn.y = y;

  parentNode.appendChild(tn);
  return { id: req.id, ok: true, result: { node_id: tn.id } };
}

function isChildrenContainer(node: BaseNode): node is BaseNode & ChildrenMixin {
  return "appendChild" in node;
}
function isLayoutMixin(node: BaseNode): node is BaseNode & LayoutMixin {
  return "resize" in node;
}
function isBlendMixin(node: BaseNode): node is BaseNode & BlendMixin {
  return "opacity" in node;
}
function isSceneNode(node: BaseNode): node is SceneNode {
  return "visible" in node;
}

async function handleUpdateNodeProperties(req: Req): Promise<Resp> {
  const p = req.params ?? {};
  const node_id = p["node_id"];
  const setRaw = p["set"];
  if (typeof node_id !== "string" || typeof setRaw !== "object" || setRaw === null) {
    return err(req.id, "invalid_params", "node_id and set object are required");
  }
  const set = setRaw as Record<string, unknown>;
  const node = await figma.getNodeByIdAsync(node_id);
  if (!node) return err(req.id, "node_not_found", `no node with id ${node_id}`);

  const applied: AppliedProps = {};

  try {
    if (isLayoutMixin(node)) {
      const sx = set["x"];
      const sy = set["y"];
      const sw = set["width"];
      const sh = set["height"];
      const sr = set["rotation"];
      if (typeof sx === "number") { node.x = sx; applied.x = sx; }
      if (typeof sy === "number") { node.y = sy; applied.y = sy; }
      if (typeof sw === "number" || typeof sh === "number") {
        const w = typeof sw === "number" ? sw : node.width;
        const h = typeof sh === "number" ? sh : node.height;
        node.resize(w, h);
        if (typeof sw === "number") applied.width = sw;
        if (typeof sh === "number") applied.height = sh;
      }
      if (typeof sr === "number") { node.rotation = sr; applied.rotation = sr; }
    }
    if (isBlendMixin(node)) {
      const so = set["opacity"];
      if (typeof so === "number") { node.opacity = so; applied.opacity = so; }
    }
    if (isSceneNode(node)) {
      const sv = set["visible"];
      if (typeof sv === "boolean") { node.visible = sv; applied.visible = sv; }
    }
    const sn = set["name"];
    if (typeof sn === "string") { node.name = sn; applied.name = sn; }
  } catch (e: unknown) {
    return err(req.id, "internal", errorMessage(e));
  }
  return { id: req.id, ok: true, result: { node_id, applied } };
}

function assertNever(x: never): never {
  throw new Error(`unreachable op: ${String(x)}`);
}

async function dispatch(req: Req): Promise<Resp> {
  try {
    switch (req.op) {
      case "set_text": return await handleSetText(req);
      case "delete_node": return await handleDeleteNode(req);
      case "create_text_node": return await handleCreateTextNode(req);
      case "update_node_properties": return await handleUpdateNodeProperties(req);
      case "ping": return { id: req.id, ok: true, result: { pong: true } };
      default: return assertNever(req.op);
    }
  } catch (e: unknown) {
    return err(req.id, "internal", errorMessage(e));
  }
}

const KNOWN_OPS: ReadonlySet<Op> = new Set<Op>([
  "set_text",
  "delete_node",
  "create_text_node",
  "update_node_properties",
  "ping",
]);

function parseReq(raw: unknown): Req | null {
  if (typeof raw !== "object" || raw === null) return null;
  const obj = raw as Record<string, unknown>;
  const id = obj["id"];
  const op = obj["op"];
  const params = obj["params"];
  if (typeof id !== "string") return null;
  if (typeof op !== "string" || !KNOWN_OPS.has(op as Op)) return null;
  if (params !== undefined && (typeof params !== "object" || params === null)) return null;
  return {
    id,
    op: op as Op,
    ...(params === undefined ? {} : { params: params as Record<string, unknown> }),
  };
}

function parseBridgeMessage(raw: unknown): BridgeMessage | null {
  if (typeof raw !== "object" || raw === null) return null;
  const obj = raw as Record<string, unknown>;
  if (obj["kind"] !== "from-bridge") return null;
  const payload = obj["payload"];
  if (typeof payload !== "string") return null;
  return { kind: "from-bridge", payload };
}

figma.ui.onmessage = async (msg: unknown) => {
  const bridge = parseBridgeMessage(msg);
  if (!bridge) return;
  let parsed: unknown;
  try {
    parsed = JSON.parse(bridge.payload);
  } catch {
    // No id to echo - drop silently. The bridge will time out and surface the error.
    return;
  }
  const req = parseReq(parsed);
  if (!req) {
    const maybeId = (typeof parsed === "object" && parsed !== null)
      ? (parsed as Record<string, unknown>)["id"]
      : undefined;
    if (typeof maybeId === "string") {
      send(err(maybeId, "invalid_params", "malformed request envelope"));
    }
    return;
  }
  const resp = await dispatch(req);
  send(resp);
};
