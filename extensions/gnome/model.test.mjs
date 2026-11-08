// SPDX-License-Identifier: AGPL-3.0-or-later

import assert from 'node:assert/strict';
import {test} from 'node:test';

import {badge, colorOf, menu, sidebarFrame} from './model.js';

test('the counter is masked at zero and red when late', () => {
    assert.deepEqual(badge({badge: {count: 0, alert: false}}), {text: '', visible: false, alert: false});
    assert.deepEqual(badge({badge: {count: 3, alert: true}}), {text: '3', visible: true, alert: true});
    assert.equal(badge(null).visible, false);
});

test('a project shows five items and says how many more', () => {
    const items = Array.from({length: 8}, (_, index) => ({
        id: `td:${index}`,
        title: `Task ${index}`,
        url: 'https://example.org',
        state_label: index === 0 ? 'à merger' : null,
        due_label: index === 0 ? 'demain' : null,
        actions: index === 1 ? ['open', 'complete'] : ['open'],
    }));
    const [project] = menu({projects: [{id: 'a', name: 'A', color: 'blue', action_count: 2, items}]});
    assert.equal(project.items.length, 5);
    assert.equal(project.more, 3);
    assert.equal(project.items[0].detail, 'à merger · demain');
    assert.equal(project.items[1].canComplete, true);
    assert.equal(project.items[0].canComplete, false);
});

test('no snapshot means an empty menu', () => {
    assert.deepEqual(menu(null), []);
});

test('colors come from the palette or a hex value, never from arbitrary text', () => {
    assert.equal(colorOf('blue'), '#3584e4');
    assert.equal(colorOf('#e5a50a'), '#e5a50a');
    assert.equal(colorOf('red; background: url(x)'), '#77767b');
});

test('the sidebar sits against the right edge of the work area', () => {
    const area = {x: 0, y: 32, width: 1920, height: 1048};
    assert.deepEqual(sidebarFrame(area, 340), {x: 1580, y: 32, width: 340, height: 1048});
});
