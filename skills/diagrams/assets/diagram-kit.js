// diagram-kit: lays out and renders diagrams declared as JSON, and checks them.
// Pages load it as a classic script; scripts/check.mjs evaluates the same file.
(function (root, factory) {
  const api = factory();
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.DiagramKit = api;
})(typeof globalThis !== 'undefined' ? globalThis : this, function () {
  'use strict';

  const TEXT = {
    label: { size: 14, weight: 500, line: 19 },
    sub: { size: 12, weight: 400, line: 16 },
    edge: { size: 12, weight: 400, line: 16 },
    group: { size: 12, weight: 500, line: 16 },
  };
  const NODE_PAD_X = 16;
  const NODE_PAD_Y = 12;
  const NODE_MIN_W = 112;
  const NODE_TEXT_MAX = 200;
  const GAP_MIN = 48;
  const OUTER_MIN = 8;
  const LANE = 8;
  const GROUP_PAD = 16;
  const GROUP_HEAD = 22;
  const MARGIN = 16;
  const BEND_COST = 4;
  const REUSE_COST = 0.5;
  const MAX_NODES = 12;
  const WIDE = 1200;
  const TONES = ['neutral', 'accent', 'teal', 'amber', 'purple', 'coral', 'blue', 'green', 'red'];
  const SIDES = { right: 0, bottom: 1, left: 2, top: 3 };
  const DI = [1, 0, -1, 0];
  const DJ = [0, 1, 0, -1];

  // ---------- text ----------
  let ctx = null;
  let family = null;
  function fontFamily() {
    if (family) return family;
    const css =
      typeof document !== 'undefined'
        ? getComputedStyle(document.documentElement).getPropertyValue('--font-sans').trim()
        : '';
    family = css || 'system-ui, -apple-system, "Segoe UI", sans-serif';
    return family;
  }
  function measure(s, f) {
    if (typeof document !== 'undefined') {
      ctx = ctx || document.createElement('canvas').getContext('2d');
      ctx.font = `${f.weight} ${f.size}px ${fontFamily()}`;
      return Math.ceil(ctx.measureText(s).width);
    }
    // Headless estimate, deliberately a little wide so checks err toward space.
    let w = 0;
    for (const ch of s) {
      const cp = ch.codePointAt(0);
      if (ch === ' ') w += 0.28;
      else if (cp >= 0x1f000) w += 1.2;
      else if ((cp >= 0x1100 && cp <= 0x115f) || (cp >= 0x2e80 && cp <= 0xa4cf) || (cp >= 0xac00 && cp <= 0xd7a3) || (cp >= 0xf900 && cp <= 0xfaff) || (cp >= 0xfe30 && cp <= 0xfe4f) || (cp >= 0xff00 && cp <= 0xff60) || (cp >= 0xffe0 && cp <= 0xffe6)) w += 1.0;
      else if (/[A-Z]/.test(ch)) w += 0.66;
      else if (/[0-9]/.test(ch)) w += 0.57;
      else if (/[a-z]/.test(ch)) w += 'iljft'.includes(ch) ? 0.3 : 'mw'.includes(ch) ? 0.84 : 0.55;
      else w += 0.45;
    }
    return Math.ceil(w * f.size * (f.weight >= 500 ? 1.05 : 1) * 1.04);
  }
  function wrap(text, f, max) {
    const lines = [];
    for (const para of String(text).split('\n')) {
      let line = '';
      for (const word of para.split(/\s+/).filter(Boolean)) {
        const next = line ? `${line} ${word}` : word;
        if (line && measure(next, f) > max) {
          lines.push(line);
          line = word;
        } else line = next;
      }
      lines.push(line);
    }
    return lines;
  }
  const words = (s) => String(s || '').split(/\s+/).filter(Boolean).length;
  const esc = (s) =>
    String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);

  // ---------- findings ----------
  // Warnings carry a code; a spec can accept one deliberately with "allow": [code].
  const WARN_CODES = ['nodes', 'tones', 'legend', 'words', 'crossings', 'width', 'group', 'label'];
  function findings(spec) {
    const list = [];
    const allow = new Set(Array.isArray(spec.allow) ? spec.allow : []);
    for (const code of allow) if (!WARN_CODES.includes(code)) list.push({ level: 'error', msg: `allow: unknown code "${code}"; codes are ${WARN_CODES.join(', ')}` });
    return {
      list,
      error: (msg) => list.push({ level: 'error', msg }),
      warn: (msg, code) =>
        list.push(allow.has(code) ? { level: 'info', msg: `${msg} (allowed: ${code})` } : { level: 'warn', msg: `${msg} [${code}]` }),
      info: (msg) => list.push({ level: 'info', msg }),
      ok: () => !list.some((f) => f.level === 'error'),
    };
  }

  function checkCommon(spec, F, items, what) {
    if (!spec.title || words(spec.title) < 3)
      F.error('diagram needs a "title": one sentence saying what it shows; it is also the accessible name');
    if (!items.length) F.error(`diagram has no ${what}`);
    const legend = Array.isArray(spec.legend) ? spec.legend : [];
    for (const l of legend) {
      if (!l.label) F.error('legend entry needs a label');
      if (l.tone !== undefined && !TONES.includes(l.tone)) F.error(`legend "${l.label}": tone "${l.tone}" is not one of ${TONES.join(', ')}`);
    }
    const used = new Set(items.map((n) => n.tone).filter((t) => t && t !== 'neutral'));
    const keyed = new Set(legend.map((l) => l.tone));
    const missing = [...used].filter((t) => !keyed.has(t));
    if (used.size > 1 && missing.length)
      F.warn(`tones ${missing.join(', ')} carry meaning but have no legend entry; add a legend or use one tone`, 'legend');
  }

  function checkSteps(spec, F, hasNode, hasEdge, edgeField) {
    (spec.steps || []).forEach((st, i) => {
      if (!st.title) F.error(`step ${i + 1} needs a title`);
      for (const id of st.nodes || []) if (!hasNode(id)) F.error(`step ${i + 1}: unknown node "${id}"`);
      for (const id of st[edgeField] || []) if (!hasEdge(id)) F.error(`step ${i + 1}: unknown ${edgeField === 'edges' ? 'edge' : 'message'} ${JSON.stringify(id)}`);
    });
  }

  function checkTones(items, F, what) {
    const used = new Set();
    for (const it of items) {
      if (it.tone === undefined) continue;
      if (!TONES.includes(it.tone)) F.error(`${what} ${it.id}: tone "${it.tone}" is not one of ${TONES.join(', ')}`);
      else if (it.tone !== 'neutral') used.add(it.tone);
    }
    if (used.size > 3)
      F.warn(`${used.size} tones in use (${[...used].join(', ')}); colour should encode at most three categories`, 'tones');
    return used;
  }

  // ---------- graph layout ----------
  function layoutGraph(spec) {
    const F = findings(spec);
    const nodes = (spec.nodes || []).map((n) => ({ ...n }));
    const byId = new Map();
    const cells = new Map();
    for (const n of nodes) {
      if (!n.id || !Number.isInteger(n.col) || !Number.isInteger(n.row) || n.col < 0 || n.row < 0) {
        F.error(`node ${JSON.stringify(n.id ?? n.label)} needs an id and non-negative integer col and row`);
        continue;
      }
      if (byId.has(n.id)) F.error(`duplicate node id "${n.id}"`);
      byId.set(n.id, n);
      const key = `${n.col},${n.row}`;
      if (cells.has(key)) F.error(`nodes "${cells.get(key).id}" and "${n.id}" share col ${n.col} row ${n.row}`);
      else cells.set(key, n);
      if (!n.label) F.error(`node "${n.id}" has no label`);
      if (words(n.sub) > 6) F.warn(`node "${n.id}": subtitle has ${words(n.sub)} words; keep it to five and move detail to "detail" or prose`, 'words');
      if (String(n.label || '').length > 32) F.warn(`node "${n.id}": label is long; name the thing, put detail elsewhere`, 'words');
    }
    if (nodes.length > MAX_NODES)
      F.warn(`${nodes.length} nodes; past ${MAX_NODES} split into an overview and detail diagrams`, 'nodes');
    checkTones(nodes, F, 'node');
    checkCommon(spec, F, nodes, 'nodes');

    const groups = (spec.groups || []).map((g) => ({ ...g, members: [] }));
    const groupById = new Map(groups.map((g) => [g.id, g]));
    checkTones(groups, F, 'group');
    for (const n of byId.values()) {
      if (n.group === undefined) continue;
      const g = groupById.get(n.group);
      if (!g) F.error(`node "${n.id}" names unknown group "${n.group}"`);
      else g.members.push(n);
    }

    const edgeIds = new Set();
    const edges = (spec.edges || []).map((e, i) => {
      let id = e.id || `${e.from}>${e.to}`;
      if (edgeIds.has(id)) {
        if (e.id) F.error(`duplicate edge id "${id}"`);
        for (let n = 2; edgeIds.has(id); n++) id = `${e.from}>${e.to}#${n}`;
      }
      edgeIds.add(id);
      return { ...e, index: i, id };
    });
    for (const e of edges) {
      if (!byId.has(e.from)) F.error(`edge ${e.id}: unknown "from" node "${e.from}"`);
      if (!byId.has(e.to)) F.error(`edge ${e.id}: unknown "to" node "${e.to}"`);
      for (const k of ['fromSide', 'toSide'])
        if (e[k] !== undefined && !(e[k] in SIDES)) F.error(`edge ${e.id}: ${k} must be one of ${Object.keys(SIDES).join(', ')}`);
      if (words(e.label) > 3) F.warn(`edge ${e.id}: label has ${words(e.label)} words; edge labels should be one to three words`, 'words');
    }
    if (!F.ok() || byId.size === 0) return { kind: 'graph', findings: F.list };

    const C = Math.max(...[...byId.values()].map((n) => n.col)) + 1;
    const R = Math.max(...[...byId.values()].map((n) => n.row)) + 1;

    for (const g of groups) {
      if (!g.members.length) {
        F.warn(`group "${g.id}" has no members`);
        continue;
      }
      g.c0 = Math.min(...g.members.map((n) => n.col));
      g.c1 = Math.max(...g.members.map((n) => n.col));
      g.r0 = Math.min(...g.members.map((n) => n.row));
      g.r1 = Math.max(...g.members.map((n) => n.row));
      for (const n of byId.values())
        if (n.group !== g.id && n.col >= g.c0 && n.col <= g.c1 && n.row >= g.r0 && n.row <= g.r1)
          F.error(`group "${g.id}" spans cols ${g.c0}-${g.c1} rows ${g.r0}-${g.r1}, which contains non-member "${n.id}"; make each group a contiguous block`);
    }
    const liveGroups = groups.filter((g) => g.members.length);
    for (let a = 0; a < liveGroups.length; a++)
      for (let b = a + 1; b < liveGroups.length; b++) {
        const g = liveGroups[a];
        const h = liveGroups[b];
        if (g.c0 <= h.c1 && h.c0 <= g.c1 && g.r0 <= h.r1 && h.r0 <= g.r1)
          F.error(`groups "${g.id}" and "${h.id}" overlap; groups cannot nest or overlap`);
      }
    if (!F.ok()) return { kind: 'graph', findings: F.list };

    // Node text and cell sizes: one width per column, one height per row.
    for (const n of byId.values()) {
      n.lines = wrap(n.label, TEXT.label, NODE_TEXT_MAX);
      n.subLines = n.sub ? wrap(n.sub, TEXT.sub, NODE_TEXT_MAX) : [];
      n.tw = Math.max(...n.lines.map((l) => measure(l, TEXT.label)), ...n.subLines.map((l) => measure(l, TEXT.sub)));
      n.th = n.lines.length * TEXT.label.line + (n.subLines.length ? 4 + n.subLines.length * TEXT.sub.line : 0);
    }
    const colW = Array.from({ length: C }, () => NODE_MIN_W);
    const rowH = Array.from({ length: R }, () => 0);
    for (const n of byId.values()) {
      colW[n.col] = Math.max(colW[n.col], n.tw + 2 * NODE_PAD_X);
      rowH[n.row] = Math.max(rowH[n.row], n.th + 2 * NODE_PAD_Y);
    }
    for (let r = 0; r < R; r++) if (!rowH[r]) rowH[r] = 40;
    for (const g of liveGroups) {
      g.labelW = g.label ? measure(g.label, TEXT.group) + 2 * 12 : 0;
      const span = colW.slice(g.c0, g.c1 + 1).reduce((a, b) => a + b, 0) + (g.c1 - g.c0) * GAP_MIN + 2 * GROUP_PAD;
      if (g.labelW > span) colW[g.c1] += g.labelW - span;
    }

    // Route on a track grid: even indices are gaps, odd indices are cell centres.
    const NI = 2 * C + 1;
    const NJ = 2 * R + 1;
    const used = new Map();
    const segKey = (i, j, ni, nj) => (i < ni || j < nj ? `${i},${j},${ni},${nj}` : `${ni},${nj},${i},${j}`);
    const cellAt = (i, j) => (i % 2 === 1 && j % 2 === 1 ? cells.get(`${(i - 1) / 2},${(j - 1) / 2}`) : undefined);

    function route(e) {
      const s = byId.get(e.from);
      const t = byId.get(e.to);
      const si = 2 * s.col + 1;
      const sj = 2 * s.row + 1;
      if (s === t) return [[si, sj], [si + 1, sj], [si + 1, sj - 1], [si, sj - 1], [si, sj]];
      const ti = 2 * t.col + 1;
      const tj = 2 * t.row + 1;
      const startDirs = e.fromSide !== undefined ? [SIDES[e.fromSide]] : [0, 1, 2, 3];
      const endDir = e.toSide !== undefined ? (SIDES[e.toSide] + 2) % 4 : undefined;
      const best = new Map();
      const prev = new Map();
      const queue = [];
      const push = (state, cost, from) => {
        if (best.has(state) && best.get(state) <= cost) return;
        best.set(state, cost);
        prev.set(state, from);
        queue.push([cost, state]);
      };
      for (const d of startDirs) {
        const ni = si + DI[d];
        const nj = sj + DJ[d];
        if (ni < 0 || nj < 0 || ni >= NI || nj >= NJ) continue;
        if (ni === ti && nj === tj) {
          if (endDir === undefined || endDir === d) return [[si, sj], [ti, tj]];
          continue;
        }
        if (cellAt(ni, nj)) continue;
        push(`${ni},${nj},${d}`, 1, null);
      }
      let goal = null;
      while (queue.length) {
        let k = 0;
        for (let q = 1; q < queue.length; q++) if (queue[q][0] < queue[k][0]) k = q;
        const [cost, state] = queue.splice(k, 1)[0];
        if (cost > best.get(state)) continue;
        const [i, j, d] = state.split(',').map(Number);
        if (i === ti && j === tj) {
          goal = state;
          break;
        }
        for (let nd = 0; nd < 4; nd++) {
          if (nd === (d + 2) % 4) continue;
          const ni = i + DI[nd];
          const nj = j + DJ[nd];
          if (ni < 0 || nj < 0 || ni >= NI || nj >= NJ) continue;
          const occupant = cellAt(ni, nj);
          if (occupant && occupant !== t) continue;
          if (occupant === t && endDir !== undefined && nd !== endDir) continue;
          const step = 1 + (nd !== d ? BEND_COST : 0) + REUSE_COST * (used.get(segKey(i, j, ni, nj)) || 0);
          push(`${ni},${nj},${nd}`, cost + step, state);
        }
      }
      if (!goal) return null;
      const pts = [];
      for (let st = goal; st; st = prev.get(st)) {
        const [i, j] = st.split(',').map(Number);
        pts.unshift([i, j]);
      }
      pts.unshift([si, sj]);
      return pts;
    }

    for (const e of edges) {
      const pts = route(e);
      if (!pts) {
        F.error(`edge ${e.id}: no route between "${e.from}" and "${e.to}" (fromSide/toSide may be impossible)`);
        continue;
      }
      for (let k = 0; k + 1 < pts.length; k++) {
        const key = segKey(pts[k][0], pts[k][1], pts[k + 1][0], pts[k + 1][1]);
        used.set(key, (used.get(key) || 0) + 1);
      }
      e.track = corners(pts);
    }
    if (!F.ok()) return { kind: 'graph', findings: F.list };

    // Gap sizes: room for parallel edges, group borders and edge labels.
    // before/after: group padding at the start and end of each gap.
    const borders = (n) => ({ before: Array(n).fill(0), after: Array(n).fill(0) });
    const gv = borders(C + 1);
    const gh = borders(R + 1);
    for (const g of liveGroups) {
      gv.after[g.c0] = GROUP_PAD;
      gv.before[g.c1 + 1] = GROUP_PAD;
      gh.after[g.r0] = GROUP_PAD + GROUP_HEAD;
      gh.before[g.r1 + 1] = GROUP_PAD;
    }
    const laneCount = (vertical) => {
      const sets = Array.from({ length: (vertical ? C : R) + 1 }, () => new Set());
      for (const e of edges)
        for (let k = 0; k + 1 < e.track.length; k++) {
          const [a, b] = [e.track[k], e.track[k + 1]];
          if (vertical && a[0] === b[0] && a[0] % 2 === 0) sets[a[0] / 2].add(slotKey(e));
          if (!vertical && a[1] === b[1] && a[1] % 2 === 0) sets[a[1] / 2].add(slotKey(e));
        }
      return sets.map((s) => s.size);
    };
    const slotKey = (e) => (spec.bus ? `from:${e.from}` : e.id);
    const laneV = laneCount(true);
    const laneH = laneCount(false);
    const need = (k) => (k ? 24 + LANE * (k - 1) : 0);
    const gapV = Array.from({ length: C + 1 }, (_, v) => {
      const inner = v === 0 || v === C ? OUTER_MIN : GAP_MIN;
      return Math.max(inner, gv.before[v] + gv.after[v] + Math.max(need(laneV[v]), v === 0 || v === C ? 0 : 16));
    });
    const gapH = Array.from({ length: R + 1 }, (_, h) => {
      const inner = h === 0 || h === R ? OUTER_MIN : GAP_MIN;
      return Math.max(inner, gh.before[h] + gh.after[h] + Math.max(need(laneH[h]), h === 0 || h === R ? 0 : 16));
    });

    for (const e of edges)
      if (e.label) {
        e.labelW = measure(e.label, TEXT.edge) + 12;
        e.labelH = TEXT.edge.line + 4;
      }

    function geometry() {
      const colX = [];
      const gapX = [];
      let x = MARGIN;
      for (let v = 0; v <= C; v++) {
        gapX[v] = x;
        x += gapV[v];
        if (v < C) {
          colX[v] = x;
          x += colW[v];
        }
      }
      const width = x + MARGIN;
      const rowY = [];
      const gapY = [];
      let y = MARGIN;
      for (let h = 0; h <= R; h++) {
        gapY[h] = y;
        y += gapH[h];
        if (h < R) {
          rowY[h] = y;
          y += rowH[h];
        }
      }
      const height = y + MARGIN;
      const tx = (i) =>
        i % 2 === 1
          ? colX[(i - 1) / 2] + colW[(i - 1) / 2] / 2
          : gapX[i / 2] + gv.before[i / 2] + (gapV[i / 2] - gv.before[i / 2] - gv.after[i / 2]) / 2;
      const ty = (j) =>
        j % 2 === 1
          ? rowY[(j - 1) / 2] + rowH[(j - 1) / 2] / 2
          : gapY[j / 2] + gh.before[j / 2] + (gapH[j / 2] - gh.before[j / 2] - gh.after[j / 2]) / 2;
      const box = (n) => ({ x: colX[n.col], y: rowY[n.row], w: colW[n.col], h: rowH[n.row] });
      function path(e) {
        const pts = e.track.map(([i, j]) => ({ x: tx(i), y: ty(j) }));
        clipToBox(pts, 0, 1, box(byId.get(e.from)));
        clipToBox(pts, pts.length - 1, pts.length - 2, box(byId.get(e.to)));
        return pts;
      }
      return { width, height, colX, rowY, box, path };
    }

    // Lay out, place labels, and widen the gap under any label that found no
    // clear spot; a few passes settle every diagram that can be settled.
    function place() {
      const geo = geometry();
      const paths = edges.map((e) => geo.path(e));
      nudge(paths, edges, slotKey, true);
      nudge(paths, edges, slotKey, false);
      const nodesOut = [...byId.values()].map((n) => ({ ...n, box: geo.box(n) }));
      const segs = paths.flatMap((pts, ei) => pts.slice(1).map((p, k) => [pts[k], p, ei]));
      const groupsOut = liveGroups.map((g) => {
        const x = geo.colX[g.c0] - GROUP_PAD;
        const y = geo.rowY[g.r0] - GROUP_PAD - GROUP_HEAD;
        const x2 = geo.colX[g.c1] + colW[g.c1] + GROUP_PAD;
        const y2 = geo.rowY[g.r1] + rowH[g.r1] + GROUP_PAD;
        // The heading goes in the top padding, in the leftmost stretch no edge crosses.
        const band = { x: x + 4, y: y + 2, w: x2 - x - 8, h: GROUP_HEAD - 2 };
        const blocks = segs
          .filter(([p, q]) => segBox(p, q, band))
          .map(([p, q]) => [Math.min(p.x, q.x) - 6, Math.max(p.x, q.x) + 6])
          .sort((a, b) => a[0] - b[0]);
        let head = null;
        let cursor = band.x;
        for (const [lo, hi] of [...blocks, [band.x + band.w, Infinity]]) {
          if (lo - cursor >= g.labelW) {
            head = { x: cursor, y: band.y, w: g.labelW, h: band.h };
            break;
          }
          cursor = Math.max(cursor, hi);
        }
        return { ...g, box: { x, y, w: x2 - x, h: y2 - y }, head: head || { x: band.x, y: band.y, w: g.labelW, h: band.h }, headBlocked: !head };
      });
      const placed = [];
      const failed = [];
      const edgesOut = edges.map((e, i) => {
        const pts = paths[i];
        if (!e.label) return { ...e, points: pts, labelBox: null };
        const lens = pts.slice(1).map((p, k) => Math.abs(p.x - pts[k].x) + Math.abs(p.y - pts[k].y));
        const order = lens.map((_, k) => k).sort((a, b) => lens[b] - lens[a]);
        // A label sits on its line (side 0, over a background), or just beside it.
        const box = (k, off, side = 0) => {
          const a = pts[k];
          const b = pts[k + 1];
          const horizontal = a.y === b.y;
          const across = side * (horizontal ? e.labelH / 2 + 4 : e.labelW / 2 + 6);
          const cx = (a.x + b.x) / 2 + (horizontal ? off : across);
          const cy = (a.y + b.y) / 2 + (horizontal ? across : off);
          return { text: e.label, x: cx - e.labelW / 2, y: cy - e.labelH / 2, w: e.labelW, h: e.labelH };
        };
        const clear = (c) =>
          !nodesOut.some((n) => overlap(c, n.box, 4)) &&
          !groupsOut.some((g) => g.label && overlap(c, g.head)) &&
          !placed.some((p) => overlap(c, p, 2)) &&
          !segs.some(([p, q, ei]) => ei !== i && segBox(p, q, c));
        let label = null;
        for (const k of order) {
          const horizontal = pts[k].y === pts[k + 1].y;
          const slack = (lens[k] - (horizontal ? e.labelW : e.labelH)) / 2 - 6;
          for (const side of [0, -1, 1])
            for (let off = 0; off <= slack && !label; off += 6)
              for (const o of off ? [off, -off] : [0])
                if (!label && clear(box(k, o, side))) label = box(k, o, side);
          if (label) break;
        }
        if (!label) {
          failed.push({ e, k: order[0] });
          label = box(order[0], 0);
        }
        placed.push(label);
        return { ...e, points: pts, labelBox: label };
      });
      return { geo, nodesOut, groupsOut, edgesOut, failed };
    }

    function widenFor({ e, k }) {
      const [a, b] = [e.track[k], e.track[k + 1]];
      const inside = (lo, hi, n) => {
        const out = [];
        for (let x = Math.min(lo, hi); x <= Math.max(lo, hi); x++) if (x % 2 === 0 && x > 0 && x < n - 1) out.push(x / 2);
        if (!out.length) for (let x = Math.min(lo, hi); x <= Math.max(lo, hi); x++) if (x % 2 === 0) out.push(x / 2);
        return out[Math.floor(out.length / 2)];
      };
      if (a[1] === b[1]) {
        if (a[1] % 2 === 0) gapH[a[1] / 2] += e.labelH + 8;
        else gapV[inside(a[0], b[0], NI)] += e.labelW + 8;
      } else if (a[0] % 2 === 0) gapV[a[0] / 2] += e.labelW + 8;
      else gapH[inside(a[1], b[1], NJ)] += e.labelH + 8;
    }

    let res = place();
    for (let pass = 0; res.failed.length && pass < 5; pass++) {
      for (const f of res.failed) widenFor(f);
      res = place();
    }
    const { geo, nodesOut, groupsOut, edgesOut } = res;
    for (const g of groupsOut)
      if (g.label && g.headBlocked)
        F.warn(`group "${g.id}": no stretch of its top edge is free of edges for the heading; shorten the label or move edges`, 'group');

    lintGraph(nodesOut, edgesOut, groupsOut, F);
    checkSteps(spec, F, (id) => byId.has(id), (id) => edgesOut.some((e) => e.id === id), 'edges');
    if (geo.width > WIDE) F.warn(`diagram is ${Math.ceil(geo.width)}px wide; past ${WIDE}px it scrolls on narrow screens; restack or split it`, 'width');
    return { kind: 'graph', width: geo.width, height: geo.height, nodes: nodesOut, edges: edgesOut, groups: groupsOut, findings: F.list };
  }

  function corners(pts) {
    const out = [pts[0]];
    for (let k = 1; k + 1 < pts.length; k++) {
      const [a, b, c] = [pts[k - 1], pts[k], pts[k + 1]];
      if ((a[0] === b[0] && b[0] === c[0]) || (a[1] === b[1] && b[1] === c[1])) continue;
      out.push(b);
    }
    out.push(pts[pts.length - 1]);
    return out;
  }

  // Move the endpoint at index k (a box centre) to the box edge facing index toward.
  function clipToBox(pts, k, toward, b) {
    const p = pts[k];
    const q = pts[toward];
    if (q.x > p.x) p.x = b.x + b.w;
    else if (q.x < p.x) p.x = b.x;
    else if (q.y > p.y) p.y = b.y + b.h;
    else p.y = b.y;
  }

  function nudge(paths, edges, slotKey, horizontal) {
    const tracks = new Map();
    paths.forEach((pts, ei) => {
      for (let k = 0; k + 1 < pts.length; k++) {
        const a = pts[k];
        const b = pts[k + 1];
        const flat = horizontal ? a.y === b.y && a.x !== b.x : a.x === b.x && a.y !== b.y;
        if (!flat) continue;
        const key = Math.round(horizontal ? a.y : a.x);
        const lo = Math.min(horizontal ? a.x : a.y, horizontal ? b.x : b.y);
        const hi = Math.max(horizontal ? a.x : a.y, horizontal ? b.x : b.y);
        if (!tracks.has(key)) tracks.set(key, []);
        tracks.get(key).push({ ei, k, lo, hi, slot: slotKey(edges[ei]) });
      }
    });
    for (const segs of tracks.values()) {
      segs.sort((a, b) => a.lo - b.lo);
      let cluster = [];
      let hi = -Infinity;
      const flush = () => {
        const slots = [...new Set(cluster.map((s) => s.slot))];
        if (slots.length > 1)
          for (const s of cluster) {
            const off = (slots.indexOf(s.slot) - (slots.length - 1) / 2) * LANE;
            const pts = paths[s.ei];
            for (const p of [pts[s.k], pts[s.k + 1]]) {
              if (horizontal) p.y += off;
              else p.x += off;
            }
          }
        cluster = [];
      };
      for (const s of segs) {
        if (cluster.length && s.lo >= hi) flush();
        cluster.push(s);
        hi = Math.max(hi, s.hi);
      }
      if (cluster.length) flush();
    }
  }

  const overlap = (a, b, pad = 0) =>
    a.x < b.x + b.w + pad && b.x < a.x + a.w + pad && a.y < b.y + b.h + pad && b.y < a.y + a.h + pad;

  function lintGraph(nodes, edges, groups, F) {
    const labels = edges.filter((e) => e.labelBox);
    for (const e of labels) {
      for (const n of nodes)
        if (overlap(e.labelBox, n.box, 4)) F.error(`edge ${e.id}: label "${e.label}" collides with node "${n.id}"; shorten it or move it into prose`);
      for (const g of groups)
        if (g.label && overlap(e.labelBox, g.head)) F.warn(`edge ${e.id}: label overlaps group "${g.id}" heading`);
    }
    for (let a = 0; a < labels.length; a++)
      for (let b = a + 1; b < labels.length; b++)
        if (overlap(labels[a].labelBox, labels[b].labelBox, 2))
          F.error(`edge labels "${labels[a].label}" (${labels[a].id}) and "${labels[b].label}" (${labels[b].id}) collide`);
    let crossings = 0;
    const segs = edges.map((e) => e.points);
    for (let a = 0; a < segs.length; a++)
      for (let b = a + 1; b < segs.length; b++)
        for (let i = 0; i + 1 < segs[a].length; i++)
          for (let j = 0; j + 1 < segs[b].length; j++) if (crosses(segs[a][i], segs[a][i + 1], segs[b][j], segs[b][j + 1])) crossings++;
    if (crossings) F.warn(`${crossings} edge crossing(s); reorder columns or rows to untangle, or allow "crossings" if they cannot be avoided`, 'crossings');
    for (const e of labels)
      for (const o of edges)
        if (o !== e && o.points.slice(1).some((p, k) => segBox(o.points[k], p, e.labelBox)))
          F.warn(`edge ${e.id}: label "${e.label}" covers part of edge ${o.id}`, 'label');
  }

  function segBox(p, q, b) {
    const x1 = Math.min(p.x, q.x);
    const x2 = Math.max(p.x, q.x);
    const y1 = Math.min(p.y, q.y);
    const y2 = Math.max(p.y, q.y);
    return x2 >= b.x && x1 <= b.x + b.w && y2 >= b.y && y1 <= b.y + b.h;
  }

  function crosses(a, b, c, d) {
    const h1 = a.y === b.y;
    const h2 = c.y === d.y;
    if (h1 === h2) return false;
    const [h, v] = h1 ? [[a, b], [c, d]] : [[c, d], [a, b]];
    const x = v[0].x;
    const y = h[0].y;
    const inH = x > Math.min(h[0].x, h[1].x) + 0.5 && x < Math.max(h[0].x, h[1].x) - 0.5;
    const inV = y > Math.min(v[0].y, v[1].y) + 0.5 && y < Math.max(v[0].y, v[1].y) - 0.5;
    return inH && inV;
  }

  // ---------- sequence layout ----------
  function layoutSequence(spec) {
    const F = findings(spec);
    const parts = (spec.participants || []).map((p) => ({ ...p }));
    const index = new Map();
    parts.forEach((p, i) => {
      if (!p.id || !p.label) F.error(`participant ${JSON.stringify(p.id)} needs id and label`);
      if (index.has(p.id)) F.error(`duplicate participant "${p.id}"`);
      index.set(p.id, i);
    });
    checkTones(parts, F, 'participant');
    checkCommon(spec, F, parts, 'participants');
    if (parts.length > 7) F.warn(`${parts.length} participants; past seven, split the sequence`, 'nodes');
    const msgs = (spec.messages || []).map((m, i) => ({ ...m, index: i }));
    if (msgs.length > 20) F.warn(`${msgs.length} messages; past twenty, split the sequence`, 'nodes');
    for (const m of msgs) {
      if (m.note !== undefined) {
        for (const id of [].concat(m.over || [])) if (!index.has(id)) F.error(`note ${m.index}: unknown participant "${id}"`);
        if (!m.over) F.error(`note ${m.index}: needs "over" (one or two participant ids)`);
        continue;
      }
      if (!index.has(m.from)) F.error(`message ${m.index}: unknown "from" "${m.from}"`);
      if (!index.has(m.to)) F.error(`message ${m.index}: unknown "to" "${m.to}"`);
      if (words(m.label) > 8) F.warn(`message ${m.index}: label has ${words(m.label)} words; keep messages short`, 'words');
    }
    if (!F.ok() || !parts.length) return { kind: 'sequence', findings: F.list };

    for (const p of parts) {
      p.lines = wrap(p.label, TEXT.label, NODE_TEXT_MAX);
      p.subLines = p.sub ? wrap(p.sub, TEXT.sub, NODE_TEXT_MAX) : [];
      p.w = Math.max(NODE_MIN_W, ...p.lines.map((l) => measure(l, TEXT.label) + 2 * NODE_PAD_X), ...p.subLines.map((l) => measure(l, TEXT.sub) + 2 * NODE_PAD_X));
      p.th = p.lines.length * TEXT.label.line + (p.subLines.length ? 4 + p.subLines.length * TEXT.sub.line : 0);
    }
    const boxH = Math.max(...parts.map((p) => p.th)) + 2 * NODE_PAD_Y;
    const gaps = parts.slice(1).map((p, i) => (parts[i].w + p.w) / 2 + 32);
    let rightExtra = 0;
    const prefix = spec.autonumber;
    for (const m of msgs) {
      if (m.note !== undefined) {
        m.lines = wrap(m.note, TEXT.sub, 240);
        m.w = Math.max(...m.lines.map((l) => measure(l, TEXT.sub))) + 20;
        continue;
      }
      const n = msgs.filter((x) => x.note === undefined).indexOf(m) + 1;
      m.text = (prefix ? `${n}. ` : '') + (m.label || '');
      m.labelW = m.text ? measure(m.text, TEXT.edge) + 12 : 0;
      const a = index.get(m.from);
      const b = index.get(m.to);
      if (a === b) {
        const needRoom = 40 + m.labelW + 8;
        if (a < gaps.length) gaps[a] = Math.max(gaps[a], needRoom + parts[a + 1].w / 2);
        else rightExtra = Math.max(rightExtra, needRoom);
        continue;
      }
      const lo = Math.min(a, b);
      const hi = Math.max(a, b);
      const span = gaps.slice(lo, hi).reduce((s, g) => s + g, 0);
      if (span < m.labelW + 32) {
        const add = (m.labelW + 32 - span) / (hi - lo);
        for (let i = lo; i < hi; i++) gaps[i] += add;
      }
    }
    const xs = [MARGIN + parts[0].w / 2];
    gaps.forEach((g, i) => xs.push(xs[i] + g));
    let width = xs[xs.length - 1] + Math.max(parts[parts.length - 1].w / 2, rightExtra) + MARGIN;
    let y = MARGIN + boxH + 32;
    for (const m of msgs) {
      if (m.note !== undefined) {
        const ids = [].concat(m.over).map((id) => index.get(id));
        const lo = Math.min(...ids);
        const hi = Math.max(...ids);
        const cx = (xs[lo] + xs[hi]) / 2;
        const w = Math.max(m.w, xs[hi] - xs[lo] + 48);
        const h = m.lines.length * TEXT.sub.line + 12;
        m.box = { x: cx - w / 2, y, w, h };
        width = Math.max(width, m.box.x + w + MARGIN);
        y += h + 20;
        continue;
      }
      const a = index.get(m.from);
      const b = index.get(m.to);
      if (a === b) {
        m.points = [{ x: xs[a], y: y + 6 }, { x: xs[a] + 36, y: y + 6 }, { x: xs[a] + 36, y: y + 26 }, { x: xs[a] + 1, y: y + 26 }];
        m.labelPos = { x: xs[a] + 44, y: y + 16, anchor: 'start' };
        y += 44;
      } else {
        const dir = b > a ? 1 : -1;
        m.points = [{ x: xs[a], y: y + 14 }, { x: xs[b] - dir, y: y + 14 }];
        m.labelPos = { x: (xs[a] + xs[b]) / 2, y: y + 6, anchor: 'middle' };
        y += 36;
      }
    }
    const height = y + 12 + MARGIN;
    let outParts = parts.map((p, i) => ({ ...p, box: { x: xs[i] - p.w / 2, y: MARGIN, w: p.w, h: boxH }, cx: xs[i] }));
    // A note wider than its participants can reach past the left edge; shift everything right.
    const minX = Math.min(...outParts.map((p) => p.box.x), ...msgs.filter((m) => m.box).map((m) => m.box.x));
    if (minX < MARGIN) {
      const dx = MARGIN - minX;
      outParts = outParts.map((p) => ({ ...p, cx: p.cx + dx, box: { ...p.box, x: p.box.x + dx } }));
      for (const m of msgs) {
        if (m.box) m.box.x += dx;
        if (m.points) for (const pt of m.points) pt.x += dx;
        if (m.labelPos) m.labelPos.x += dx;
      }
      width += dx;
    }
    checkSteps(spec, F, (id) => index.has(id), (i) => Number.isInteger(i) && i >= 0 && i < msgs.length, 'messages');
    if (width > WIDE) F.warn(`diagram is ${Math.ceil(width)}px wide; past ${WIDE}px it scrolls on narrow screens; fewer participants or shorter labels`, 'width');
    return { kind: 'sequence', width, height, participants: outParts, messages: msgs, lifelineEnd: y + 12, findings: F.list };
  }

  function layout(spec) {
    if (!spec || typeof spec !== 'object') return { findings: [{ level: 'error', msg: 'diagram spec must be a JSON object' }] };
    const type = spec.type || 'graph';
    if (type === 'graph') return layoutGraph(spec);
    if (type === 'sequence') return layoutSequence(spec);
    return { findings: [{ level: 'error', msg: `unknown diagram type "${type}"; use "graph" or "sequence"` }] };
  }

  // ---------- SVG ----------
  function roundedPath(pts, r = 6) {
    let d = `M${pts[0].x},${pts[0].y}`;
    for (let k = 1; k < pts.length; k++) {
      const p = pts[k];
      if (k === pts.length - 1) {
        d += ` L${p.x},${p.y}`;
        break;
      }
      const a = pts[k - 1];
      const b = pts[k + 1];
      const r1 = Math.min(r, Math.hypot(p.x - a.x, p.y - a.y) / 2, Math.hypot(b.x - p.x, b.y - p.y) / 2);
      const ux = Math.sign(p.x - a.x);
      const uy = Math.sign(p.y - a.y);
      const vx = Math.sign(b.x - p.x);
      const vy = Math.sign(b.y - p.y);
      d += ` L${p.x - ux * r1},${p.y - uy * r1} Q${p.x},${p.y} ${p.x + vx * r1},${p.y + vy * r1}`;
    }
    return d;
  }

  function textLines(lines, x, top, f, cls) {
    return lines
      .map((l, i) => `<text class="${cls}" x="${x}" y="${top + i * f.line + f.line * 0.72}" text-anchor="middle">${esc(l)}</text>`)
      .join('');
  }

  const nodeAttrs = (n) =>
    n.detail
      ? `data-node="${esc(n.id)}" tabindex="0" role="button" aria-label="${esc(n.label)}${n.sub ? `: ${esc(n.sub)}` : ''}, show details"`
      : `data-node="${esc(n.id)}" aria-hidden="true"`;

  // Plain-text structure of a diagram, in reading order.
  function describe(lay, spec) {
    const name = (id) => {
      const list = lay.kind === 'graph' ? lay.nodes : lay.participants;
      const n = list.find((x) => x.id === id);
      return n ? n.label : id;
    };
    if (lay.kind === 'graph') {
      const items = lay.nodes.map((n) => {
        const out = lay.edges.filter((e) => e.from === n.id).map((e) => `${name(e.to)}${e.label ? ` (${e.label})` : ''}${e.style === 'dashed' ? ', dashed' : ''}`);
        const group = n.group ? ` In ${(lay.groups.find((g) => g.id === n.group) || {}).label || n.group}.` : '';
        return `<li>${esc(n.label)}${n.sub ? `, ${esc(n.sub)}` : ''}.${esc(group)}${out.length ? ` Leads to ${esc(out.join('; '))}.` : ''}</li>`;
      });
      return `<p>${esc(spec.title)}</p><ul>${items.join('')}</ul>`;
    }
    const items = lay.messages.map((m) =>
      m.note !== undefined
        ? `<li>Note over ${esc([].concat(m.over).map(name).join(' and '))}: ${esc(m.note)}</li>`
        : `<li>${esc(name(m.from))} to ${esc(name(m.to))}: ${esc(m.text || '')}${m.style === 'reply' ? ' (reply)' : ''}</li>`,
    );
    return `<p>${esc(spec.title)}</p><ol>${items.join('')}</ol>`;
  }

  function nodeSvg(n, box, attrs) {
    const tone = n.tone || 'neutral';
    const top = box.y + (box.h - n.th) / 2;
    const cx = box.x + box.w / 2;
    let s = `<g class="dk-node dk-tone-${tone}${n.detail ? ' dk-clickable' : ''}" ${attrs}>`;
    s += `<rect x="${box.x}" y="${box.y}" width="${box.w}" height="${box.h}" rx="6"/>`;
    s += textLines(n.lines, cx, top, TEXT.label, 'dk-t');
    if (n.subLines.length) s += textLines(n.subLines, cx, top + n.lines.length * TEXT.label.line + 4, TEXT.sub, 'dk-ts');
    return s + '</g>';
  }

  // Marker ids are per diagram: one page holds many SVGs, and fragment ids must be unique.
  let renders = 0;
  const markers = (uid) =>
    '<defs>' +
    ['edge', 'hi'].map((k) => `<marker id="dk-${uid}-${k}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M1,1 L9,5 L1,9 z" class="dk-head-${k}"/></marker>`).join('') +
    '</defs>';

  // The SVG is a labelled group so its focusable boxes stay reachable; the
  // full structure is also given as text (describe) for assistive technology.
  function render(lay, spec, opts = {}) {
    const uid = `${String(spec.id || 'd').replace(/[^a-zA-Z0-9_-]/g, '')}${++renders}`;
    const arrow = `url(#dk-${uid}-edge)`;
    const w = Math.ceil(lay.width);
    const open = `<svg class="dk-svg" data-uid="${uid}" viewBox="0 0 ${w} ${Math.ceil(lay.height)}" width="${w}" style="min-width:${Math.round(w * 0.75)}px" role="group" aria-label="${esc(spec.title)}"${opts.describedBy ? ` aria-describedby="${opts.describedBy}"` : ''}><title>${esc(spec.title)}</title>${markers(uid)}`;
    if (lay.kind === 'graph') {
      let s = open;
      for (const g of lay.groups)
        s += `<g class="dk-group dk-tone-${g.tone || 'neutral'}"><rect x="${g.box.x}" y="${g.box.y}" width="${g.box.w}" height="${g.box.h}" rx="10"/>${g.label ? `<text class="dk-tg" x="${g.head.x + 8}" y="${g.box.y + 17}">${esc(g.label)}</text>` : ''}</g>`;
      for (const e of lay.edges)
        s += `<path class="dk-edge${e.style === 'dashed' ? ' dk-dashed' : ''}" data-edge="${esc(e.id)}" data-from="${esc(e.from)}" data-to="${esc(e.to)}" d="${roundedPath(e.points)}" marker-end="${arrow}"/>`;
      for (const e of lay.edges)
        if (e.labelBox) {
          const b = e.labelBox;
          s += `<g class="dk-elabel" data-edge="${esc(e.id)}"><rect x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" rx="3"/><text class="dk-ts" x="${b.x + b.w / 2}" y="${b.y + b.h / 2 + 4}" text-anchor="middle">${esc(b.text)}</text></g>`;
        }
      for (const n of lay.nodes) s += nodeSvg(n, n.box, nodeAttrs(n));
      return s + '</svg>';
    }
    let s = open;
    for (const p of lay.participants)
      s += `<line class="dk-lifeline" x1="${p.cx}" y1="${p.box.y + p.box.h}" x2="${p.cx}" y2="${lay.lifelineEnd}"/>`;
    for (const m of lay.messages) {
      if (m.note !== undefined) {
        const b = m.box;
        s += `<g class="dk-note" data-msg="${m.index}"><rect x="${b.x}" y="${b.y}" width="${b.w}" height="${b.h}" rx="4"/>${m.lines
          .map((l, i) => `<text class="dk-ts" x="${b.x + b.w / 2}" y="${b.y + 6 + i * TEXT.sub.line + 12}" text-anchor="middle">${esc(l)}</text>`)
          .join('')}</g>`;
        continue;
      }
      s += `<g class="dk-msg" data-msg="${m.index}"><path class="dk-edge${m.style === 'reply' || m.style === 'dashed' ? ' dk-dashed' : ''}" d="${roundedPath(m.points, 4)}" marker-end="${arrow}"/>`;
      if (m.text) s += `<text class="dk-ts dk-mlabel" x="${m.labelPos.x}" y="${m.labelPos.y}" text-anchor="${m.labelPos.anchor}">${esc(m.text)}</text>`;
      s += '</g>';
    }
    for (const p of lay.participants) s += nodeSvg(p, p.box, nodeAttrs(p));
    return s + '</svg>';
  }

  // ---------- browser behaviour ----------
  function content(ref) {
    if (!ref) return '';
    if (ref.startsWith('#')) {
      const el = document.querySelector(ref);
      return el ? (el.tagName === 'TEMPLATE' ? el.innerHTML : el.outerHTML) : `<p class="dk-missing">missing ${esc(ref)}</p>`;
    }
    return ref;
  }

  let mounts = 0;
  function mount(script) {
    let spec;
    const fig = document.createElement('figure');
    fig.className = 'dk-figure';
    try {
      spec = JSON.parse(script.textContent);
    } catch (err) {
      fig.innerHTML = `<div class="dk-findings"><b>Diagram JSON does not parse:</b> ${esc(err.message)}</div>`;
      script.after(fig);
      return;
    }
    const lay = layout(spec);
    const errors = lay.findings.filter((f) => f.level === 'error');
    for (const f of lay.findings) if (f.level !== 'info') console[f.level === 'error' ? 'error' : 'warn'](`[diagram ${spec.id || ''}] ${f.msg}`);
    const target = script.dataset.target ? document.getElementById(script.dataset.target) : null;
    if (target) target.appendChild(fig);
    else script.after(fig);
    if (errors.length) {
      fig.innerHTML = `<div class="dk-findings"><b>Diagram ${esc(spec.id || '')} has errors:</b><ul>${errors.map((f) => `<li>${esc(f.msg)}</li>`).join('')}</ul></div>`;
      return;
    }
    const items = lay.kind === 'graph' ? lay.nodes : lay.participants;
    const hasDetail = items.some((n) => n.detail);
    const descId = `dk-desc-${++mounts}`;
    fig.innerHTML = `<div class="dk-stage${hasDetail ? ' dk-with-detail' : ''}"><div class="dk-canvas">${render(lay, spec, { describedBy: descId })}<div class="dk-sr" id="${descId}">${describe(lay, spec)}</div></div>${hasDetail ? `<aside class="dk-detail" aria-live="polite">${spec.detailHint ? `<p class="dk-hint">${esc(spec.detailHint)}</p>` : '<p class="dk-hint">Select a box for details.</p>'}</aside>` : ''}</div>${spec.legend ? legend(spec.legend) : ''}${spec.caption ? `<figcaption>${esc(spec.caption)}</figcaption>` : ''}`;
    const svg = fig.querySelector('svg');
    const head = (on) => `url(#dk-${svg.dataset.uid}-${on ? 'hi' : 'edge'})`;
    const aside = fig.querySelector('.dk-detail');
    const byId = new Map(items.map((n) => [n.id, n]));

    function highlight(nodeIds, edgeIds) {
      const active = nodeIds.size || edgeIds.size;
      svg.classList.toggle('dk-focus', !!active);
      svg.querySelectorAll('[data-node]').forEach((el) => el.classList.toggle('dk-on', nodeIds.has(el.dataset.node)));
      svg.querySelectorAll('.dk-edge[data-edge], .dk-elabel').forEach((el) => {
        const on = edgeIds.has(el.dataset.edge);
        el.classList.toggle('dk-on', on);
        if (el.tagName === 'path') el.setAttribute('marker-end', head(on));
      });
      svg.querySelectorAll('[data-msg]').forEach((el) => {
        const on = edgeIds.has(`msg:${el.dataset.msg}`);
        el.classList.toggle('dk-on', on);
        const p = el.querySelector('path');
        if (p) p.setAttribute('marker-end', head(on));
      });
    }
    function select(id) {
      const n = byId.get(id);
      if (!n) return;
      const nodesOn = new Set([id]);
      const edgesOn = new Set();
      if (lay.kind === 'graph')
        for (const e of lay.edges)
          if (e.from === id || e.to === id) {
            edgesOn.add(e.id);
            nodesOn.add(e.from);
            nodesOn.add(e.to);
          }
      highlight(nodesOn, edgesOn);
      if (aside) aside.innerHTML = `<h4>${esc(n.label)}</h4>${n.sub ? `<p class="dk-sub">${esc(n.sub)}</p>` : ''}${content(n.detail)}`;
    }
    svg.addEventListener('click', (ev) => {
      const g = ev.target.closest('[data-node]');
      if (g) select(g.dataset.node);
      else if (!fig.querySelector('.dk-steps')) highlight(new Set(), new Set());
    });
    svg.addEventListener('keydown', (ev) => {
      const g = ev.target.closest('[data-node]');
      if (g && (ev.key === 'Enter' || ev.key === ' ')) {
        ev.preventDefault();
        select(g.dataset.node);
      }
    });
    if (spec.steps && spec.steps.length) steps(fig, spec, lay, highlight);
  }

  function legend(items) {
    return `<div class="dk-legend">${items.map((l) => `<span class="dk-key dk-tone-${esc(l.tone || 'neutral')}"><i></i>${esc(l.label)}</span>`).join('')}</div>`;
  }

  function steps(fig, spec, lay, highlight) {
    const list = spec.steps;
    const box = document.createElement('div');
    box.className = 'dk-steps';
    box.tabIndex = 0;
    fig.appendChild(box);
    let cur = 0;
    const edgeRef = (r) => (typeof r === 'number' ? `msg:${r}` : r);
    function show(i) {
      cur = Math.max(0, Math.min(list.length - 1, i));
      const st = list[cur];
      const nodes = new Set(st.nodes || []);
      const edges = new Set((st.edges || st.messages || []).map(edgeRef));
      if (lay.kind === 'graph')
        for (const e of lay.edges) if (edges.has(e.id)) {
          nodes.add(e.from);
          nodes.add(e.to);
        }
      highlight(nodes, edges);
      box.innerHTML = `<div class="dk-step-head"><button type="button" data-d="-1" ${cur === 0 ? 'disabled' : ''}>Previous</button><span>Step ${cur + 1} of ${list.length}</span><button type="button" data-d="1" ${cur === list.length - 1 ? 'disabled' : ''}>Next</button></div><h4>${esc(st.title || '')}</h4><div class="dk-step-body">${content(st.body)}</div>`;
    }
    box.addEventListener('click', (ev) => {
      const b = ev.target.closest('button[data-d]');
      if (b) show(cur + Number(b.dataset.d));
    });
    box.addEventListener('keydown', (ev) => {
      if (ev.key === 'ArrowRight' || ev.key === 'j') show(cur + 1);
      if (ev.key === 'ArrowLeft' || ev.key === 'k') show(cur - 1);
    });
    show(0);
  }

  function page() {
    const nav = document.querySelector('nav[data-toc]');
    if (nav && !nav.children.length) {
      for (const sec of document.querySelectorAll('main section[id]')) {
        const h = sec.querySelector('h2');
        if (!h) continue;
        const a = document.createElement('a');
        a.href = `#${sec.id}`;
        a.textContent = h.textContent;
        nav.appendChild(a);
      }
    }
    document.addEventListener('click', (ev) => {
      const chip = ev.target.closest('.src');
      if (!chip || !navigator.clipboard) return;
      navigator.clipboard.writeText(chip.textContent.trim()).then(() => {
        chip.classList.add('dk-copied');
        setTimeout(() => chip.classList.remove('dk-copied'), 900);
      });
    });
  }

  function renderAll() {
    page();
    document.querySelectorAll('script[type="application/json"][data-diagram]').forEach(mount);
  }

  if (typeof document !== 'undefined') {
    if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', renderAll);
    else renderAll();
  }

  return { layout, render, TONES };
});
