// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

const assert = require('node:assert/strict');
const fs = require('node:fs');
const djot = require('../third_party/djot.js/lib/index.js');

// Ignore wrapping and list tightness, but retain attributes, destinations,
// list type/start, code contents and every block boundary.
function normalize(node) {
  if (Array.isArray(node)) {
    const result = [];
    for (const child of node.map(normalize)) {
      const previous = result.at(-1);
      if (child.tag === 'str' && previous?.tag === 'str' &&
          !child.attributes && !previous.attributes) {
        previous.text += child.text;
      } else {
        result.push(child);
      }
    }
    for (const child of result) {
      if (child.tag === 'str') child.text = child.text.replace(/[ \t\r\n]+/g, ' ');
    }
    return result;
  }
  if (node && typeof node === 'object') {
    if (node.tag === 'soft_break') return {tag: 'str', text: ' '};
    const result = {};
    for (const [key, value] of Object.entries(node)) {
      if (key !== 'pos' && key !== 'tight') result[key] = normalize(value);
    }
    return result;
  }
  return node;
}

const {source, formatted} = JSON.parse(fs.readFileSync(0, 'utf8'));
assert.deepStrictEqual(normalize(djot.parse(formatted)), normalize(djot.parse(source)));
