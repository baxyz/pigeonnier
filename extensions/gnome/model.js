// SPDX-License-Identifier: AGPL-3.0-or-later

// What the menu shows, from the daemon's snapshot. No GNOME import here, so that it can be tested
// outside of the Shell.

const ITEMS_PER_PROJECT = 5;

/** The counter of the top bar: masked at zero, red only when something is late. */
export function badge(snapshot) {
    const count = snapshot?.badge?.count ?? 0;
    return {
        text: count > 0 ? String(count) : '',
        visible: count > 0,
        alert: Boolean(snapshot?.badge?.alert),
    };
}

/** One section per project, with its first items and how many more there are. */
export function menu(snapshot) {
    return (snapshot?.projects ?? []).map(project => {
        const items = project.items ?? [];
        return {
            id: project.id,
            name: project.name,
            color: project.color,
            actionCount: project.action_count ?? 0,
            items: items.slice(0, ITEMS_PER_PROJECT).map(item => ({
                id: item.id,
                title: item.title,
                detail: [item.state_label, item.due_label].filter(Boolean).join(' · '),
                url: item.url,
                urgency: item.due_urgency ?? null,
                canComplete: (item.actions ?? []).includes('complete'),
            })),
            more: Math.max(0, items.length - ITEMS_PER_PROJECT),
        };
    });
}

const NAMED_COLORS = {
    blue: '#3584e4',
    green: '#2ec27e',
    yellow: '#e5a50a',
    orange: '#ff7800',
    red: '#e01b24',
    purple: '#9141ac',
    brown: '#986a44',
    gray: '#77767b',
};

/** A color for a style, from a palette name or a hex value. Anything else falls back to gray. */
export function colorOf(color) {
    if (NAMED_COLORS[color])
        return NAMED_COLORS[color];
    return /^#[0-9a-fA-F]{3,8}$/.test(color ?? '') ? color : NAMED_COLORS.gray;
}

/** Where the sidebar goes in a work area: against the right edge, full height. */
export function sidebarFrame(workArea, width) {
    return {
        x: workArea.x + workArea.width - width,
        y: workArea.y,
        width,
        height: workArea.height,
    };
}
