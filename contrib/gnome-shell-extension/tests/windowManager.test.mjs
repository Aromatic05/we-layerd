import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

const source = await readFile(new URL('../we-layerd@aromatic/windowManager.js', import.meta.url), 'utf8');

class WindowStub {
    constructor({fullscreen = false, nativeWindowList = true} = {}) {
        this.fullscreen = fullscreen;
        this.above = true;
        this.minimized = false;
        this.hiddenFromWindowList = false;
        this.restoreCount = 0;
        this.signals = new Map();
        this.nextSignal = 1;
        this.lowerCount = 0;
        if (!nativeWindowList) {
            this.hide_from_window_list = undefined;
            this.show_in_window_list = undefined;
        }
    }

    connect(signal, callback) {
        const id = this.nextSignal++;
        this.signals.set(id, {signal, callback});
        return id;
    }

    connect_after(signal, callback) { return this.connect(signal, callback); }
    disconnect(id) { this.signals.delete(id); }

    emit(signal) {
        for (const handler of [...this.signals.values()]) {
            if (handler.signal === signal)
                handler.callback(this);
        }
    }

    unmake_fullscreen() { this.fullscreen = false; this.emit('notify::fullscreen'); }
    unmake_above() { this.above = false; }
    unminimize() { this.minimized = false; this.emit('notify::minimized'); }
    stick() { this.sticky = true; }
    lower() { ++this.lowerCount; }
    hide_from_window_list() { this.hiddenFromWindowList = true; }
    show_in_window_list() { this.hiddenFromWindowList = false; ++this.restoreCount; }
    get_title() { return 'Renderer'; }
    get_pid() { return 123; }
}

async function harness(windows, matches) {
    let nextTimeout = 1;
    const timeouts = new Map();
    const GLib = {
        PRIORITY_DEFAULT: 0,
        SOURCE_REMOVE: false,
        timeout_add(_priority, _delay, callback) {
            const id = nextTimeout++;
            timeouts.set(id, callback);
            return id;
        },
        source_remove(id) { return timeouts.delete(id); },
    };
    const shellSignals = new WindowStub();
    const context = vm.createContext({
        console: {log() {}},
        global: {
            window_manager: shellSignals,
            get_window_actors: () => windows.map(window => ({get_meta_window: () => window})),
        },
    });
    const glib = new vm.SyntheticModule(['default'], function () {
        this.setExport('default', GLib);
    }, {context});
    const module = new vm.SourceTextModule(source, {context, identifier: 'windowManager.js'});
    await module.link(specifier => {
        assert.equal(specifier, 'gi://GLib');
        return glib;
    });
    await module.evaluate();
    const manager = new module.namespace.WindowManager(matches);
    manager.enable();
    return {
        manager,
        timeouts,
        flushLower() {
            const pending = [...timeouts.values()];
            timeouts.clear();
            for (const callback of pending)
                callback();
        },
        showDesktop() {
            // Mutter excludes windows hidden through its native window-list API.
            // Simulate that policy; this is not a compositor integration test.
            for (const window of windows) {
                if (!window.hiddenFromWindowList) {
                    window.minimized = true;
                    window.emit('notify::minimized');
                }
            }
        },
    };
}

test('managed fullscreen windows keep shell chrome available, including later fullscreen requests', async () => {
    const wallpaper = new WindowStub({fullscreen: true});
    const ordinary = new WindowStub({fullscreen: true});
    const shell = await harness([wallpaper, ordinary], window => window === wallpaper);
    assert.equal(wallpaper.fullscreen, false);
    assert.equal(ordinary.fullscreen, true);
    assert.equal(wallpaper.above, false);
    assert.equal(wallpaper.sticky, true);
    wallpaper.fullscreen = true;
    wallpaper.emit('notify::fullscreen');
    assert.equal(wallpaper.fullscreen, false);
    shell.flushLower();
    assert.equal(wallpaper.lowerCount, 1);
    shell.manager.disable();
});

test('show desktop excludes the renderer while minimizing ordinary windows', async () => {
    const wallpaper = new WindowStub();
    const ordinary = new WindowStub();
    const shell = await harness([wallpaper, ordinary], window => window === wallpaper);
    assert.equal(wallpaper.hiddenFromWindowList, true);
    assert.equal(ordinary.hiddenFromWindowList, false);
    shell.showDesktop();
    assert.equal(wallpaper.minimized, false);
    assert.equal(ordinary.minimized, true);
    wallpaper.hiddenFromWindowList = false;
    wallpaper.emit('shown');
    assert.equal(wallpaper.hiddenFromWindowList, true);
    shell.manager.disable();
    assert.equal(wallpaper.hiddenFromWindowList, false);
    assert.equal(wallpaper.restoreCount, 1);
    assert.equal(ordinary.restoreCount, 0);
});

test('older shells without the native hide API still enforce fullscreen and minimize recovery', async () => {
    const wallpaper = new WindowStub({fullscreen: true, nativeWindowList: false});
    const shell = await harness([wallpaper], () => true);
    assert.equal(wallpaper.fullscreen, false);
    shell.showDesktop();
    assert.equal(wallpaper.minimized, false);
    shell.manager.disable();
});

test('disable and unregister disconnect callbacks and cancel deferred lowering', async () => {
    const wallpaper = new WindowStub();
    let matches = true;
    const shell = await harness([wallpaper], () => matches);
    assert.equal(shell.timeouts.size, 1);
    matches = false;
    shell.manager.refreshMatches();
    assert.equal(wallpaper.signals.size, 0);
    assert.equal(shell.timeouts.size, 0);
    assert.equal(wallpaper.managed, null);
    assert.equal(wallpaper.hiddenFromWindowList, false);
    assert.equal(wallpaper.restoreCount, 1);
    matches = true;
    shell.manager.refreshMatches();
    assert.equal(wallpaper.hiddenFromWindowList, true);
    shell.manager.disable();
    assert.equal(wallpaper.hiddenFromWindowList, false);
    assert.equal(wallpaper.restoreCount, 2);
    assert.equal(wallpaper.signals.size, 0);
    assert.equal(shell.timeouts.size, 0);
    wallpaper.fullscreen = true;
    wallpaper.emit('notify::fullscreen');
    shell.flushLower();
    assert.equal(wallpaper.fullscreen, true);
    assert.equal(wallpaper.lowerCount, 0);
});
