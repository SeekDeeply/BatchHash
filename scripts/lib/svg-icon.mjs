// Minimal SVG rasterizer for the BatchHash app icon.
//
// Platform scaffolding is generated in CI (flutter create), so the icon is
// painted once here and the resulting PNGs are committed. Only the SVG subset
// this icon uses is supported: linear gradients, cubic/quadratic beziers,
// polylines, circles, rounded rects, strokes, group opacity, translate and one
// rounded-rect clip. Unsupported constructs are skipped rather than guessed, so
// a redesigned icon degrades visibly instead of silently.
//
// Fills are rasterized with scanline coverage (fast and properly antialiased);
// strokes use a per-pixel distance test inside the shape's bounding box.

import { deflateSync } from 'node:zlib';

const SUB = 2; // sub-scanlines per pixel row

function crc32(buf) {
  let crc = ~0;
  for (let i = 0; i < buf.length; i += 1) {
    crc ^= buf[i];
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return ~crc >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([length, body, crc]);
}

/** Encode straight RGBA bytes as a PNG. */
export function encodePng(width, height, rgba) {
  const stride = width * 4 + 1;
  const raw = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    raw[y * stride] = 0; // filter: none
    rgba.copy(raw, y * stride + 1, y * width * 4, (y + 1) * width * 4);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

function attrs(tag) {
  const out = {};
  const re = /([\w:-]+)\s*=\s*"([^"]*)"/g;
  let match;
  while ((match = re.exec(tag)) !== null) out[match[1]] = match[2];
  return out;
}

function parseColor(value, opacity) {
  if (!value || value === 'none' || !value.startsWith('#')) return null;
  const alpha = opacity === undefined || opacity === '' ? 1 : Number(opacity);
  const body = value.slice(1);
  const full = body.length === 3
    ? body.split('').map((c) => c + c).join('')
    : body;
  return {
    r: parseInt(full.slice(0, 2), 16),
    g: parseInt(full.slice(2, 4), 16),
    b: parseInt(full.slice(4, 6), 16),
    a: alpha,
  };
}

/** Flatten a path d attribute into subpaths of points. */
function flattenPath(d) {
  const tokens = d.match(/[MmLlHhVvCcSsQqTtZz]|-?\d*\.?\d+(?:e-?\d+)?/gi) ?? [];
  const subpaths = [];
  let current = [];
  let x = 0;
  let y = 0;
  let startX = 0;
  let startY = 0;
  let lastCubic = null;
  let lastQuad = null;
  let command = 'M';
  let index = 0;
  const take = () => Number(tokens[index++]);

  const cubicTo = (p1, p2, p3) => {
    const from = [x, y];
    for (let step = 1; step <= 20; step += 1) {
      const t = step / 20;
      const mt = 1 - t;
      current.push([
        mt * mt * mt * from[0] + 3 * mt * mt * t * p1[0] +
          3 * mt * t * t * p2[0] + t * t * t * p3[0],
        mt * mt * mt * from[1] + 3 * mt * mt * t * p1[1] +
          3 * mt * t * t * p2[1] + t * t * t * p3[1],
      ]);
    }
    x = p3[0];
    y = p3[1];
  };

  const quadTo = (p1, p2) => {
    const from = [x, y];
    for (let step = 1; step <= 20; step += 1) {
      const t = step / 20;
      const mt = 1 - t;
      current.push([
        mt * mt * from[0] + 2 * mt * t * p1[0] + t * t * p2[0],
        mt * mt * from[1] + 2 * mt * t * p1[1] + t * t * p2[1],
      ]);
    }
    x = p2[0];
    y = p2[1];
  };

  while (index < tokens.length) {
    const token = tokens[index];
    if (/^[MmLlHhVvCcSsQqTtZz]$/.test(token)) {
      command = token;
      index += 1;
      if (command === 'Z' || command === 'z') {
        if (current.length > 1) {
          current.push([startX, startY]);
          subpaths.push(current);
        }
        current = [];
        x = startX;
        y = startY;
        lastCubic = null;
        lastQuad = null;
        continue;
      }
    }
    if (index >= tokens.length) break;
    const relative = command === command.toLowerCase();

    if (command === 'M' || command === 'm') {
      const px = take();
      const py = take();
      x = relative ? x + px : px;
      y = relative ? y + py : py;
      if (current.length > 1) subpaths.push(current);
      current = [[x, y]];
      startX = x;
      startY = y;
      command = relative ? 'l' : 'L';
      lastCubic = null;
      lastQuad = null;
    } else if (command === 'L' || command === 'l') {
      const px = take();
      const py = take();
      x = relative ? x + px : px;
      y = relative ? y + py : py;
      current.push([x, y]);
      lastCubic = null;
      lastQuad = null;
    } else if (command === 'H' || command === 'h') {
      const px = take();
      x = relative ? x + px : px;
      current.push([x, y]);
      lastCubic = null;
      lastQuad = null;
    } else if (command === 'V' || command === 'v') {
      const py = take();
      y = relative ? y + py : py;
      current.push([x, y]);
      lastCubic = null;
      lastQuad = null;
    } else if (command === 'C' || command === 'c') {
      const x1 = take();
      const y1 = take();
      const x2 = take();
      const y2 = take();
      const px = take();
      const py = take();
      const p1 = relative ? [x + x1, y + y1] : [x1, y1];
      const p2 = relative ? [x + x2, y + y2] : [x2, y2];
      const p3 = relative ? [x + px, y + py] : [px, py];
      cubicTo(p1, p2, p3);
      lastCubic = p2;
      lastQuad = null;
    } else if (command === 'S' || command === 's') {
      const x2 = take();
      const y2 = take();
      const px = take();
      const py = take();
      // The first control point mirrors the previous cubic's second one.
      const p1 = lastCubic
        ? [2 * x - lastCubic[0], 2 * y - lastCubic[1]]
        : [x, y];
      const p2 = relative ? [x + x2, y + y2] : [x2, y2];
      const p3 = relative ? [x + px, y + py] : [px, py];
      cubicTo(p1, p2, p3);
      lastCubic = p2;
      lastQuad = null;
    } else if (command === 'Q' || command === 'q') {
      const x1 = take();
      const y1 = take();
      const px = take();
      const py = take();
      const p1 = relative ? [x + x1, y + y1] : [x1, y1];
      const p2 = relative ? [x + px, y + py] : [px, py];
      quadTo(p1, p2);
      lastQuad = p1;
      lastCubic = null;
    } else if (command === 'T' || command === 't') {
      const px = take();
      const py = take();
      const p1 = lastQuad
        ? [2 * x - lastQuad[0], 2 * y - lastQuad[1]]
        : [x, y];
      const p2 = relative ? [x + px, y + py] : [px, py];
      quadTo(p1, p2);
      lastQuad = p1;
      lastCubic = null;
    } else {
      index += 1;
    }
  }
  if (current.length > 1) subpaths.push(current);
  return subpaths;
}

function circlePath(cx, cy, r) {
  const points = [];
  for (let step = 0; step <= 96; step += 1) {
    const angle = (step / 96) * Math.PI * 2;
    points.push([cx + Math.cos(angle) * r, cy + Math.sin(angle) * r]);
  }
  return [points];
}

function roundedRectPath(x, y, w, h, r) {
  const points = [];
  const corners = [
    [x + w - r, y + r, -Math.PI / 2, 0],
    [x + w - r, y + h - r, 0, Math.PI / 2],
    [x + r, y + h - r, Math.PI / 2, Math.PI],
    [x + r, y + r, Math.PI, (Math.PI * 3) / 2],
  ];
  for (const [ax, ay, from, to] of corners) {
    for (let step = 0; step <= 12; step += 1) {
      const angle = from + ((to - from) * step) / 12;
      points.push([ax + Math.cos(angle) * r, ay + Math.sin(angle) * r]);
    }
  }
  points.push(points[0]);
  return [points];
}

function distanceToSegment(px, py, ax, ay, bx, by) {
  const dx = bx - ax;
  const dy = by - ay;
  const lengthSquared = dx * dx + dy * dy;
  const t = lengthSquared === 0
    ? 0
    : Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / lengthSquared));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}

function inRoundedClip(x, y, rx, ry, rw, rh, rr) {
  if (x < rx || y < ry || x > rx + rw || y > ry + rh) return false;
  if (x >= rx + rr && x <= rx + rw - rr) return true;
  if (y >= ry + rr && y <= ry + rh - rr) return true;
  const cx = Math.min(Math.max(x, rx + rr), rx + rw - rr);
  const cy = Math.min(Math.max(y, ry + rr), ry + rh - rr);
  return Math.hypot(x - cx, y - cy) <= rr;
}

function sampleGradient(stops, t) {
  if (!stops || stops.length === 0) return { r: 0, g: 0, b: 0, a: 1 };
  if (t <= stops[0].offset) return stops[0].color;
  const last = stops[stops.length - 1];
  if (t >= last.offset) return last.color;
  for (let i = 0; i < stops.length - 1; i += 1) {
    const a = stops[i];
    const b = stops[i + 1];
    if (t >= a.offset && t <= b.offset) {
      const span = b.offset - a.offset || 1;
      const k = (t - a.offset) / span;
      return {
        r: a.color.r + (b.color.r - a.color.r) * k,
        g: a.color.g + (b.color.g - a.color.g) * k,
        b: a.color.b + (b.color.b - a.color.b) * k,
        a: a.color.a + (b.color.a - a.color.a) * k,
      };
    }
  }
  return last.color;
}

function parseGradients(svg) {
  const gradients = new Map();
  const gradRe = /<linearGradient[^>]*\bid="([^"]+)"[^>]*>([\s\S]*?)<\/linearGradient>/g;
  let gradMatch;
  while ((gradMatch = gradRe.exec(svg)) !== null) {
    const stops = [];
    const stopRe = /<stop\b([^>]*?)\/?>/g;
    let stopMatch;
    while ((stopMatch = stopRe.exec(gradMatch[2])) !== null) {
      const sa = attrs(stopMatch[1]);
      stops.push({
        offset: Number(sa.offset ?? 0),
        color: parseColor(sa['stop-color'], sa['stop-opacity']) ??
          { r: 0, g: 0, b: 0, a: 1 },
      });
    }
    stops.sort((a, b) => a.offset - b.offset);
    gradients.set(gradMatch[1], stops);
  }
  return gradients;
}

// Presentation properties that inherit from an element to its children.
const INHERITED = [
  'fill', 'stroke', 'stroke-width', 'stroke-opacity', 'stroke-linecap',
  'stroke-linejoin', 'fill-opacity', 'fill-rule',
];

function emptyStyle() {
  return Object.create(null);
}

/** Merge a parent style with the attributes declared on [a]. */
function inheritStyle(parent, a) {
  const style = Object.create(null);
  for (const key of INHERITED) {
    if (a[key] !== undefined) style[key] = a[key];
    else if (parent[key] !== undefined) style[key] = parent[key];
  }
  return style;
}

/**
 * Collect draw operations in document order, with inherited style applied.
 *
 * squareCorners paints rounded rects as plain rects. The artwork's background
 * is a rounded rect, so this turns the icon into a full-bleed square for
 * platforms that apply their own rounded mask (iOS, macOS).
 */
function parseOps(svg, gradients, squareCorners) {
  const ops = [];
  const stack = [{
    opacity: 1,
    translate: [0, 0],
    style: emptyStyle(),
    clip: false,
  }];
  const elementRe = /<(\/?)(path|circle|rect|g)\b([^>]*?)(\/?)>/g;
  let match;
  while ((match = elementRe.exec(svg)) !== null) {
    const [, closing, tag, rest, selfClosing] = match;
    const a = attrs(rest);
    const top = stack[stack.length - 1];

    if (tag === 'g') {
      if (closing === '/') {
        if (stack.length > 1) stack.pop();
        continue;
      }
      if (selfClosing === '/') continue;
      const translate = [...top.translate];
      if (a.transform) {
        const tr = /translate\(\s*(-?[\d.]+)[ ,]+(-?[\d.]+)\s*\)/.exec(a.transform);
        if (tr) {
          translate[0] += Number(tr[1]);
          translate[1] += Number(tr[2]);
        }
      }
      stack.push({
        opacity: top.opacity * (a.opacity ? Number(a.opacity) : 1),
        translate,
        clip: top.clip || Boolean(a['clip-path']),
        style: inheritStyle(top.style, a),
      });
      continue;
    }

    let subpaths;
    if (tag === 'path') {
      subpaths = flattenPath(a.d ?? '');
    } else if (tag === 'circle') {
      subpaths = circlePath(Number(a.cx ?? 0), Number(a.cy ?? 0), Number(a.r ?? 0));
    } else {
      const rx = Number(a.x ?? 0);
      const ry = Number(a.y ?? 0);
      const rw = Number(a.width ?? 0);
      const rh = Number(a.height ?? 0);
      const rr = squareCorners ? 0 : Number(a.rx ?? 0);
      if (rw <= 0 || rh <= 0) continue;
      subpaths = rr > 0
        ? roundedRectPath(rx, ry, rw, rh, rr)
        : [[[rx, ry], [rx + rw, ry], [rx + rw, ry + rh], [rx, ry + rh]]];
    }
    if (subpaths.length === 0) continue;
    const dx = top.translate[0];
    const dy = top.translate[1];
    if (dx !== 0 || dy !== 0) {
      subpaths = subpaths.map((points) =>
        points.map(([x, y]) => [x + dx, y + dy]));
    }

    const style = inheritStyle(top.style, a);
    // Opacity accumulates multiplicatively: ancestor groups are already folded
    // into top.opacity, so only this element's own value is applied here.
    const opacity = top.opacity * Number(a.opacity ?? 1);

    const fillValue = style.fill;
    const strokeValue = style.stroke;

    if (fillValue && fillValue !== 'none') {
      const gradientId = /^url\(#(.+)\)$/.exec(fillValue);
      ops.push({
        kind: 'fill',
        subpaths,
        alpha: opacity * Number(style['fill-opacity'] ?? 1),
        gradient: gradientId ? gradients.get(gradientId[1]) : null,
        solid: gradientId ? null : parseColor(fillValue),
        clip: top.clip,
      });
    }
    if (strokeValue && strokeValue !== 'none') {
      ops.push({
        kind: 'stroke',
        subpaths,
        alpha: opacity * Number(style['stroke-opacity'] ?? 1),
        width: Number(style['stroke-width'] ?? 1),
        solid: parseColor(strokeValue),
        clip: top.clip,
      });
    }
  }
  return ops;
}

function bounds(subpaths) {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const points of subpaths) {
    for (const [x, y] of points) {
      if (x < minX) minX = x;
      if (y < minY) minY = y;
      if (x > maxX) maxX = x;
      if (y > maxY) maxY = y;
    }
  }
  return [minX, minY, maxX, maxY];
}

/**
 * Coverage of a filled polygon set, sampled per pixel row.
 *
 * Only the shape's bounding box is scanned. At 1024px the icon has ~20 shapes
 * and a full-canvas scan per shape makes the render take about a minute, which
 * is far too slow to run in every CI job.
 */
function fillCoverage(subpaths, size, scale, clip, bbox) {
  const coverage = new Float32Array(size * size);
  const row = new Float32Array(size * SUB);
  const xs = [];
  const rowFrom = Math.max(0, Math.floor(bbox[1] * scale));
  const rowTo = Math.min(size, Math.ceil(bbox[3] * scale));
  for (let py = rowFrom; py < rowTo; py += 1) {
    row.fill(0);
    let touched = false;
    for (let sy = 0; sy < SUB; sy += 1) {
      const yUser = ((py * SUB + sy) + 0.5) / (scale * SUB);
      xs.length = 0;
      for (const points of subpaths) {
        for (let i = 0, j = points.length - 1; i < points.length; j = i, i += 1) {
          const [xi, yi] = points[i];
          const [xj, yj] = points[j];
          if ((yi > yUser) !== (yj > yUser)) {
            xs.push(xi + ((yUser - yi) / (yj - yi)) * (xj - xi));
          }
        }
      }
      if (xs.length < 2) continue;
      xs.sort((a, b) => a - b);
      for (let k = 0; k + 1 < xs.length; k += 2) {
        let xa = xs[k] * scale * SUB;
        let xb = xs[k + 1] * scale * SUB;
        if (xb <= 0 || xa >= size * SUB) continue;
        xa = Math.max(0, xa);
        xb = Math.min(size * SUB, xb);
        if (xb <= xa) continue;
        touched = true;
        const ia = Math.floor(xa);
        const ib = Math.floor(xb);
        if (ia === ib) {
          row[ia] += xb - xa;
        } else {
          row[ia] += ia + 1 - xa;
          for (let m = ia + 1; m < ib; m += 1) row[m] += 1;
          row[ib] += xb - ib;
        }
      }
    }
    if (!touched) continue;
    const colFrom = Math.max(0, Math.floor(bbox[0] * scale));
    const colTo = Math.min(size, Math.ceil(bbox[2] * scale));
    for (let px = colFrom; px < colTo; px += 1) {
      let acc = 0;
      for (let m = 0; m < SUB; m += 1) acc += row[px * SUB + m];
      const value = acc / (SUB * SUB);
      if (value <= 0) continue;
      if (clip && !inRoundedClip((px + 0.5) / scale, (py + 0.5) / scale, 72, 72, 880, 880, 210)) {
        continue;
      }
      coverage[py * size + px] = Math.min(1, value);
    }
  }
  return coverage;
}

/**
 * Accumulate stroke coverage into [buffer], one segment bounding box at a time.
 *
 * Testing every pixel of the whole stroke box against every segment is what
 * makes the naive version take a minute at 1024px; walking each segment's own
 * small neighbourhood keeps the cost proportional to the painted area.
 */
function strokeCoverage(op, size, scale, buffer) {
  buffer.fill(0);
  const half = op.width / 2;
  const pad = Math.ceil(half * scale) + 2;
  const samples = SUB * SUB;
  let painted = false;

  for (const points of op.subpaths) {
    for (let i = 0; i < points.length - 1; i += 1) {
      const [ax, ay] = points[i];
      const [bx, by] = points[i + 1];
      const x0 = Math.max(0, Math.floor(Math.min(ax, bx) * scale) - pad);
      const y0 = Math.max(0, Math.floor(Math.min(ay, by) * scale) - pad);
      const x1 = Math.min(size, Math.ceil(Math.max(ax, bx) * scale) + pad);
      const y1 = Math.min(size, Math.ceil(Math.max(ay, by) * scale) + pad);
      for (let py = y0; py < y1; py += 1) {
        for (let px = x0; px < x1; px += 1) {
          let hits = 0;
          for (let sy = 0; sy < SUB; sy += 1) {
            for (let sx = 0; sx < SUB; sx += 1) {
              const ux = (px + (sx + 0.5) / SUB) / scale;
              const uy = (py + (sy + 0.5) / SUB) / scale;
              if (op.clip && !inRoundedClip(ux, uy, 72, 72, 880, 880, 210)) continue;
              if (distanceToSegment(ux, uy, ax, ay, bx, by) <= half) hits += 1;
            }
          }
          if (hits === 0) continue;
          const value = hits / samples;
          const index = py * size + px;
          if (value > buffer[index]) buffer[index] = value;
          painted = true;
        }
      }
    }
  }
  return painted;
}

/**
 * Rasterize the icon SVG into RGBA pixels at [size]x[size].
 *
 * options.roundedClip = false paints the artwork without its rounded-corner
 * mask, so the background gradient runs edge to edge. Apple's icon sets need
 * this: the OS applies its own mask and rejects an alpha channel.
 */
export function renderIcon(svg, size, options = {}) {
  const roundedClip = options.roundedClip ?? true;
  const gradients = parseGradients(svg);
  let ops = parseOps(svg, gradients, !roundedClip);

  if (!roundedClip) {
    // The artwork is inset in its 1024 viewBox (the background starts at 72 and
    // stops at 952). For a full-bleed square the whole canvas must be painted,
    // so the background fill is extended to the canvas edges.
    const background = ops.find((op) => op.kind === 'fill' && op.gradient) ??
      ops.find((op) => op.kind === 'fill');
    if (!background) throw new Error('icon has no background fill to extend');
    // Everything drawn inside the rounded clip is the artwork itself. The only
    // operation outside it is the decorative border ring, which would show up as
    // a hard frame the platform mask does not have -- so it is dropped.
    // The artwork occupies the middle 880 of the 1024 viewBox because the
    // rounded-corner variant needs that margin. A full-bleed square has no such
    // margin, so the artwork is scaled up about the canvas centre (512, 512).
    const ZOOM = 1.14;
    const zoom = (op) => ({
      ...op,
      subpaths: op.subpaths.map((points) => points.map(([x, y]) => [
        512 + (x - 512) * ZOOM,
        512 + (y - 512) * ZOOM,
      ])),
      ...(op.width !== undefined ? { width: op.width * ZOOM } : {}),
    });
    const inside = ops.filter((op) => op !== background && op.clip).map(zoom);
    ops = [
      {
        ...background,
        subpaths: [[[0, 0], [1024, 0], [1024, 1024], [0, 1024]]],
        // The whole point: no rounded clip, so the gradient reaches the corners.
        clip: false,
      },
      ...inside,
    ];
  }
  const pixels = Buffer.alloc(size * size * 4);
  const strokeBuffer = new Float32Array(size * size);
  const scale = size / 1024;

  const paintBuffer = (buffer, color, alpha) => {
    for (let py = 0; py < size; py += 1) {
      for (let px = 0; px < size; px += 1) {
        const value = buffer[py * size + px];
        if (value <= 0) continue;
        composite(px, py, color, alpha * value);
      }
    }
  };

  const composite = (px, py, color, alpha) => {
    const index = (py * size + px) * 4;
    const srcA = Math.min(1, alpha) * (color.a ?? 1);
    if (srcA <= 0) return;
    const dstA = pixels[index + 3] / 255;
    const outA = srcA + dstA * (1 - srcA);
    if (outA <= 0) return;
    const channels = [color.r, color.g, color.b];
    for (let c = 0; c < 3; c += 1) {
      pixels[index + c] = Math.round(
        (channels[c] * srcA + pixels[index + c] * dstA * (1 - srcA)) / outA,
      );
    }
    pixels[index + 3] = Math.round(outA * 255);
  };

  for (const op of ops) {
    if (op.kind === 'fill') {
      const [bx0, by0, bx1, by1] = bounds(op.subpaths);
      const coverage = fillCoverage(op.subpaths, size, scale, op.clip, [bx0, by0, bx1, by1]);
      const rowFrom = Math.max(0, Math.floor(by0 * scale));
      const rowTo = Math.min(size, Math.ceil(by1 * scale));
      const colFrom = Math.max(0, Math.floor(bx0 * scale));
      const colTo = Math.min(size, Math.ceil(bx1 * scale));
      for (let py = rowFrom; py < rowTo; py += 1) {
        for (let px = colFrom; px < colTo; px += 1) {
          const cov = coverage[py * size + px];
          if (cov <= 0) continue;
          const color = op.gradient
            ? sampleGradient(op.gradient, gradientT(bx0, by0, bx1, by1, (px + 0.5) / scale, (py + 0.5) / scale))
            : op.solid;
          if (!color) continue;
          composite(px, py, color, op.alpha * cov);
        }
      }
    } else if (op.solid) {
      if (!strokeCoverage(op, size, scale, strokeBuffer)) continue;
      paintBuffer(strokeBuffer, op.solid, op.alpha);
    }
  }
  return pixels;
}

function gradientT(bx0, by0, bx1, by1, x, y) {
  const tx = (x - bx0) / Math.max(1e-6, bx1 - bx0);
  const ty = (y - by0) / Math.max(1e-6, by1 - by0);
  return Math.max(0, Math.min(1, (tx + ty) / 2));
}

/**
 * Rasterize an opaque, full-bleed square icon.
 *
 * iOS and macOS draw their own rounded mask and reject an icon that carries an
 * alpha channel, so their icon sets are painted with the artwork's own rounded
 * clip disabled: the background gradient then covers the corners naturally and
 * no compositing trick is needed.
 *
 * [padding] insets the artwork inside the square, which is what Android's
 * adaptive launcher needs -- it only shows the middle of the foreground layer.
 */
export function renderIconSquare(svg, size, options = {}) {
  const { padding = 0 } = options;
  const pixels = renderIcon(svg, size, { roundedClip: false });

  // Force full opacity: the icon must not carry an alpha channel.
  for (let index = 3; index < pixels.length; index += 4) pixels[index] = 255;

  if (padding <= 0) return pixels;

  // Re-centre a smaller copy over a background sampled from the artwork, so the
  // inset area continues the same gradient instead of showing a hard border.
  const inner = Math.max(1, Math.round(size * (1 - padding * 2)));
  const offset = Math.round((size - inner) / 2);
  const out = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const index = (y * size + x) * 4;
      const sx = Math.min(size - 1, Math.max(0, Math.round(
        ((x - offset) / inner) * (size - 1),
      )));
      const sy = Math.min(size - 1, Math.max(0, Math.round(
        ((y - offset) / inner) * (size - 1),
      )));
      const si = (sy * size + sx) * 4;
      out[index] = pixels[si];
      out[index + 1] = pixels[si + 1];
      out[index + 2] = pixels[si + 2];
      out[index + 3] = 255;
    }
  }
  return out;
}

/** Rasterize the icon SVG to an RGBA PNG buffer. */
export function iconPng(svg, size) {
  return encodePng(size, size, renderIcon(svg, size));
}

/** Rasterize an opaque, full-bleed square icon (iOS, macOS). */
export function iconPngSquare(svg, size, options = {}) {
  const pixels = renderIconSquare(svg, size, options);
  return encodePng(size, size, pixels);
}
