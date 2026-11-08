// SPDX-License-Identifier: AGPL-3.0-or-later

// A client of the daemon, with no token and no sync logic: a counter, a menu, and the sidebar placed
// against the edge of the screen, which only the Shell may do under Wayland.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {badge, colorOf, menu, sidebarFrame} from './model.js';

const APP_WM_CLASS = 'io.github.baxyz.Pigeonnier.Gnome';
const BUS_NAME = 'io.github.baxyz.Pigeonnier';
const OBJECT_PATH = '/io/github/baxyz/Pigeonnier';
const INTERFACE = `
<node>
  <interface name="io.github.baxyz.Pigeonnier1">
    <method name="GetSnapshot"><arg type="s" direction="out"/></method>
    <method name="Refresh"/>
    <method name="Complete"><arg type="s" direction="in"/></method>
    <method name="ShowWindow"><arg type="s" direction="in"/></method>
    <signal name="Changed"><arg type="u"/></signal>
    <signal name="WindowRequested"><arg type="s"/></signal>
  </interface>
</node>`;
const DaemonProxy = Gio.DBusProxy.makeProxyWrapper(INTERFACE);

const Indicator = GObject.registerClass(
class Indicator extends PanelMenu.Button {
    _init(extension) {
        super._init(0.0, 'Pigeonnier');
        this._extension = extension;
        const box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        box.add_child(new St.Icon({
            icon_name: 'mail-unread-symbolic',
            style_class: 'system-status-icon',
        }));
        this._count = new St.Label({
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'pigeonnier-count',
            visible: false,
        });
        box.add_child(this._count);
        this.add_child(box);
        this.draw(null);
    }

    draw(snapshot) {
        const counter = badge(snapshot);
        this._count.text = counter.text;
        this._count.visible = counter.visible;
        if (counter.alert)
            this._count.add_style_class_name('pigeonnier-alert');
        else
            this._count.remove_style_class_name('pigeonnier-alert');

        this.menu.removeAll();
        for (const project of menu(snapshot))
            this._addProject(project);
        if (!snapshot) {
            const away = new PopupMenu.PopupMenuItem('Le démon ne répond pas', {reactive: false});
            this.menu.addMenuItem(away);
        }
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        const open = new PopupMenu.PopupMenuItem('Ouvrir la sidebar');
        open.connect('activate', () => this._extension.showSidebar());
        this.menu.addMenuItem(open);
    }

    _addProject(project) {
        const count = project.actionCount > 0 ? ` (${project.actionCount})` : '';
        const section = new PopupMenu.PopupSubMenuMenuItem(`${project.name}${count}`);
        const dot = new St.Widget({
            style: `background-color: ${colorOf(project.color)}; border-radius: 99px; width: 8px; height: 8px;`,
            y_align: Clutter.ActorAlign.CENTER,
        });
        section.insert_child_at_index(dot, 0);
        for (const item of project.items)
            section.menu.addMenuItem(this._itemRow(item));
        if (project.more > 0) {
            const more = new PopupMenu.PopupMenuItem(`+${project.more}`);
            more.connect('activate', () => this._extension.showSidebar());
            section.menu.addMenuItem(more);
        }
        this.menu.addMenuItem(section);
    }

    _itemRow(item) {
        const label = item.detail ? `${item.title} — ${item.detail}` : item.title;
        const row = new PopupMenu.PopupMenuItem(label);
        row.label.clutter_text.ellipsize = 3;
        row.connect('activate', () => {
            if (item.url)
                Gio.AppInfo.launch_default_for_uri(item.url, null);
        });
        if (item.canComplete) {
            const done = new St.Button({
                child: new St.Icon({icon_name: 'object-select-symbolic', icon_size: 14}),
                style_class: 'pigeonnier-done',
                accessible_name: 'Marquer comme fait',
                x_expand: true,
                x_align: Clutter.ActorAlign.END,
            });
            done.connect('clicked', () => this._extension.complete(item.id));
            row.add_child(done);
        }
        return row;
    }
});

export default class PigeonnierExtension extends Extension {
    enable() {
        this._settings = this.getSettings();
        this._indicator = new Indicator(this);
        Main.panel.addToStatusArea(this.uuid, this._indicator);

        this._proxy = new DaemonProxy(Gio.DBus.session, BUS_NAME, OBJECT_PATH, (_proxy, error) => {
            if (error)
                console.warn(`Pigeonnier: ${error.message}`);
            this._load();
        }, null, Gio.DBusProxyFlags.DO_NOT_AUTO_START);
        this._changedId = this._proxy.connectSignal('Changed', () => this._load());
        this._requestedId = this._proxy.connectSignal('WindowRequested', (_proxy, _sender, [mode]) => {
            this._wantSidebar = mode === 'sidebar';
        });
        this._ownerId = this._proxy.connect('notify::g-name-owner', () => this._load());

        this._windowId = global.display.connect('window-created', (_display, window) => {
            this._onWindowCreated(window);
        });

        Main.wm.addKeybinding(
            'toggle-sidebar',
            this._settings,
            Meta.KeyBindingFlags.NONE,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW,
            () => this._toggleSidebar()
        );
    }

    disable() {
        Main.wm.removeKeybinding('toggle-sidebar');
        global.display.disconnect(this._windowId);
        this._proxy.disconnectSignal(this._changedId);
        this._proxy.disconnectSignal(this._requestedId);
        this._proxy.disconnect(this._ownerId);
        if (this._placeId) {
            GLib.source_remove(this._placeId);
            this._placeId = null;
        }
        this._proxy = null;
        this._indicator.destroy();
        this._indicator = null;
        this._settings = null;
    }

    _load() {
        if (!this._proxy?.g_name_owner) {
            this._indicator?.draw(null);
            return;
        }
        this._proxy.GetSnapshotRemote(([text], error) => {
            if (!this._indicator)
                return;
            if (error) {
                console.warn(`Pigeonnier: ${error.message}`);
                this._indicator.draw(null);
                return;
            }
            this._indicator.draw(JSON.parse(text));
        });
    }

    complete(id) {
        this._proxy?.CompleteRemote(id, (_result, error) => {
            if (error)
                Main.notifyError('Pigeonnier', error.message);
        });
    }

    showSidebar() {
        this._wantSidebar = true;
        const window = this._findWindow();
        if (window) {
            this._place(window);
            window.activate(global.get_current_time());
            return;
        }
        this._proxy?.ShowWindowRemote('sidebar', (_result, error) => {
            if (error)
                Main.notifyError('Pigeonnier', error.message);
        });
    }

    _toggleSidebar() {
        const window = this._findWindow();
        if (window?.has_focus()) {
            window.minimize();
            return;
        }
        if (window?.minimized)
            window.unminimize();
        this.showSidebar();
    }

    _findWindow() {
        return global.get_window_actors()
            .map(actor => actor.get_meta_window())
            .find(window => window.get_wm_class() === APP_WM_CLASS) ?? null;
    }

    _onWindowCreated(window) {
        if (!this._wantSidebar)
            return;
        // The class is only known once the window has its first state: wait for the next idle.
        this._placeId = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._placeId = null;
            if (window.get_wm_class() === APP_WM_CLASS)
                this._place(window);
            return GLib.SOURCE_REMOVE;
        });
    }

    _place(window) {
        const area = window.get_work_area_current_monitor();
        const frame = sidebarFrame(area, this._settings.get_int('sidebar-width'));
        if (window.get_maximized?.())
            window.unmaximize(Meta.MaximizeFlags.BOTH);
        window.move_resize_frame(false, frame.x, frame.y, frame.width, frame.height);
    }
}
