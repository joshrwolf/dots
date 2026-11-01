#!/usr/bin/env node
// Checks a page built with this skill: every data-diagram spec lays out without
// errors, the kit files sit beside the page, and no text-art diagrams remain.
// Usage: node check.mjs <page.html>...   Exits 1 on any error or warning; a spec
// accepts a specific warning on purpose with "allow": [code].
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const kitPath = join(dirname(fileURLToPath(import.meta.url)), '..', 'assets', 'diagram-kit.js');
const sandbox = { module: { exports: {} } };
vm.runInNewContext(readFileSync(kitPath, 'utf8'), sandbox, { filename: kitPath });
const kit = sandbox.module.exports;

const pages = process.argv.slice(2);
if (!pages.length) {
  console.error('usage: node check.mjs <page.html>...');
  process.exit(2);
}

let failed = false;
for (const page of pages) {
  const html = readFileSync(page, 'utf8');
  const out = [];
  const add = (level, where, msg) => out.push({ level, where, msg });

  for (const asset of ['diagram-kit.js', 'kit.css']) {
    const linked = html.includes(`"${asset}"`) || html.includes(`'${asset}'`);
    if (!linked) add('error', 'page', `does not load ${asset}`);
    else if (!existsSync(join(dirname(page), asset))) add('error', 'page', `${asset} is not beside the page; copy it from the skill's assets/`);
  }

  for (const m of html.matchAll(/<pre[^>]*>([\s\S]*?)<\/pre>/g))
    if (/[─│┌┐└┘├┤┬┴┼▶◀▲▼]|-->|==>/.test(m[1])) add('warn', 'page', 'a <pre> block looks like a text-art diagram; draw it as a diagram spec');

  const specs = [...html.matchAll(/<script\b([^>]*\bdata-diagram\b[^>]*)>([\s\S]*?)<\/script>/g)];
  if (!specs.length) add('info', 'page', 'no data-diagram specs found');
  specs.forEach((m, i) => {
    let spec;
    try {
      spec = JSON.parse(m[2]);
    } catch (err) {
      add('error', `diagram #${i + 1}`, `JSON does not parse: ${err.message}`);
      return;
    }
    const where = `diagram ${spec.id || `#${i + 1}`}`;
    if (!/type=["']application\/json["']/.test(m[1])) add('error', where, 'script tag needs type="application/json"');
    const lay = kit.layout(spec);
    for (const f of lay.findings) add(f.level, where, f.msg);
    if (lay.width) add('info', where, `${Math.ceil(lay.width)}×${Math.ceil(lay.height)}px`);
  });

  console.log(page);
  for (const f of out) console.log(`  ${f.level.padEnd(5)} ${f.where}: ${f.msg}`);
  if (out.some((f) => f.level === 'error' || f.level === 'warn')) failed = true;
}
process.exit(failed ? 1 : 0);
