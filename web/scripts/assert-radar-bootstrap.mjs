#!/usr/bin/env node
/**
 * Fail closed if the Vite build output lost the W-Radar platform bootstrap.
 *
 * Source of truth: web/index.html (formal code W-W-AUTOML-C06A4011).
 * Stale gitignored web/dist/index.html previously shipped to auto.ml without
 * platform/bootstrap.js, leaving the frontend blind to Radar.
 *
 * Usage:
 *   node scripts/assert-radar-bootstrap.mjs
 *   node scripts/assert-radar-bootstrap.mjs dist/index.html
 *   node scripts/assert-radar-bootstrap.mjs --live https://auto.ml/
 */
import { readFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const FORMAL_CODE = 'W-W-AUTOML-C06A4011';
const BOOTSTRAP_NEEDLE = 'platform/bootstrap.js';
const CODE_NEEDLE = `data-code="${FORMAL_CODE}"`;

const __dirname = dirname(fileURLToPath(import.meta.url));
const webRoot = resolve(__dirname, '..');

function fail(msg) {
  console.error(`assert-radar-bootstrap: FAIL — ${msg}`);
  process.exit(1);
}

function checkHtml(label, html) {
  if (!html.includes(BOOTSTRAP_NEEDLE)) {
    fail(`${label} missing ${BOOTSTRAP_NEEDLE}`);
  }
  if (!html.includes(CODE_NEEDLE)) {
    fail(`${label} missing ${CODE_NEEDLE}`);
  }
  console.log(`assert-radar-bootstrap: OK — ${label} has bootstrap + ${FORMAL_CODE}`);
}

const args = process.argv.slice(2);

if (args[0] === '--live') {
  const url = args[1] || 'https://auto.ml/';
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) fail(`live fetch ${url} → HTTP ${res.status}`);
  const html = await res.text();
  checkHtml(url, html);
  process.exit(0);
}

const target = resolve(webRoot, args[0] || 'dist/index.html');
if (!existsSync(target)) {
  fail(`${target} does not exist (run vite build first)`);
}
checkHtml(target, readFileSync(target, 'utf8'));
