const {readFileSync} = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const {test} = require('node:test');

const source = readFileSync(`${__dirname}/../we-layerd@aromatic/extension.js`, 'utf8')
    .replace(/^import .*;\n/gm, '')
    .replace('export default class WeLayerdExtension', 'class WeLayerdExtension');

const activeWorkspace = {
    // eDP-1 (index 0) has a 32px top bar; HDMI-A-1 (index 3) has none.
    get_work_area_for_monitor: index => (index === 3
        ? {x: 1920, y: 0, width: 2560, height: 1440}
        : {x: 0, y: 32, width: 1920, height: 1048}),
};

function window(options = {}) {
    return {
        get_pid: () => options.pid ?? 10,
        get_title: () => options.title ?? 'App',
        get_wm_class: () => options.wmClass ?? 'app',
        get_window_type: () => options.type ?? 0,
        get_monitor: () => options.monitor ?? 0,
        located_on_workspace: workspace => workspace === activeWorkspace && options.active !== false,
        get_client_type: () => options.client ?? 0,
        showing_on_its_workspace: () => options.visible !== false,
        maximized_horizontally: options.maximized ?? false,
        maximized_vertically: options.maximized ?? false,
        is_fullscreen: () => options.fullscreen ?? false,
        get_frame_rect: () => options.frame ?? {x: 100, y: 100, width: 800, height: 600},
    };
}

function states(windows, focus = windows[0], target = {pid: 50, title: 'Renderer', wmClass: 'renderer'}, locked = false) {
    const context = vm.createContext({
        Main: {sessionMode: {isLocked: locked}},
        Extension: class {}, Meta: {
            WindowType: {NORMAL: 0, DESKTOP: 1, DOCK: 2, DIALOG: 3, MODAL_DIALOG: 4},
            WindowClientType: {WAYLAND: 0, X11: 1},
        },
        global: {
            display: {
                list_all_windows: () => windows,
                get_focus_window: () => focus,
                get_monitor_geometry: index => (index === 3
                    ? {x: 1920, y: 0, width: 2560, height: 1440}
                    : {x: 0, y: 0, width: 1920, height: 1080}),
            },
            workspace_manager: {get_active_workspace: () => activeWorkspace},
            backend: {get_monitor_manager: () => ({get_logical_monitors: () => [
                {get_number: () => 3, get_monitors: () => [{get_connector: () => 'HDMI-A-1'}]},
                {get_number: () => 0, get_monitors: () => [{get_connector: () => 'eDP-1'}]},
            ]})},
        },
    });
    const extension = vm.runInContext(`${source}\nnew WeLayerdExtension()`, context);
    extension._targets = new Map(target ? [[1, target]] : []);
    return JSON.parse(JSON.stringify(extension.GetWindowStates()));
}

test('logical monitor numbers resolve to connectors even when array order differs', () => {
    const focused = window({maximized: true});
    const other = window({monitor: 3, fullscreen: true});
    assert.deepEqual(states([focused, other], focused), [
        ['eDP-1', true, true, false], ['HDMI-A-1', false, false, true],
    ]);
});

test('inactive workspaces, minimized windows, renderer and non-normal windows are excluded', () => {
    const normal = window();
    const sticky = window({pid: 11}); // located_on_workspace includes sticky windows.
    const hidden = window({active: false, maximized: true});
    const minimized = window({visible: false, fullscreen: true});
    const renderer = window({pid: 50, title: 'Renderer', wmClass: 'renderer'});
    assert.deepEqual(states([normal, sticky, hidden, minimized, renderer, window({type: 1}), window({type: 2})], normal), [
        ['eDP-1', true, false, false], ['eDP-1', false, false, false],
    ]);
});

test('unregistered renderer does not publish rule activity', () => {
    assert.deepEqual(states([window()], null, null), []);
});

test('focus on application dialogs remains active for focused-window rules', () => {
    for (const type of [3, 4]) {
        const parent = window();
        const dialog = window({pid: 11, type});
        assert.deepEqual(states([parent, dialog], dialog), [
            ['eDP-1', false, false, false], ['eDP-1', true, false, false],
        ]);
    }
});

test('a borderless X11 window covering its whole monitor counts as fullscreen', () => {
    const borderless = window({client: 1, monitor: 3, frame: {x: 1920, y: 0, width: 2560, height: 1440}});
    const almost = window({frame: {x: 0, y: 32, width: 1920, height: 1048}});
    assert.deepEqual(states([borderless, almost], borderless), [
        // `almost` fills eDP-1's work area exactly, so it reads as maximized.
        ['HDMI-A-1', true, false, true], ['eDP-1', false, true, false],
    ]);
});

test('no window pauses or mutes the wallpaper while the screen is locked', () => {
    const game = window({fullscreen: true});
    assert.deepEqual(states([game], game, undefined, true), []);
});

test('a maximized window on a monitor without a top bar is maximized, not a game', () => {
    const maximized = window({client: 1, monitor: 3, maximized: true,
        frame: {x: 1920, y: 0, width: 2560, height: 1440}});
    assert.deepEqual(states([maximized], maximized), [['HDMI-A-1', true, true, false]]);
});

test('a window tiled to fill the work area counts as maximized', () => {
    const tiled = window({frame: {x: 8, y: 40, width: 1904, height: 1032}});
    const half = window({frame: {x: 0, y: 32, width: 960, height: 1048}});
    assert.deepEqual(states([tiled, half], tiled), [
        ['eDP-1', true, true, false], ['eDP-1', false, false, false],
    ]);
});

test('only X11 windows covering a monitor are treated as borderless games', () => {
    const wayland = window({monitor: 3, frame: {x: 1920, y: 0, width: 2560, height: 1440}});
    assert.deepEqual(states([wayland], wayland), [['HDMI-A-1', true, true, false]]);
});

test('right-click forwarding opens the background menu of the connector monitor', () => {
    const opened = [];
    const menu = {isOpen: false, open: () => opened.push('open'), close: () => opened.push('close')};
    const context = vm.createContext({
        Extension: class {},
        Meta: {WindowType: {NORMAL: 0}, WindowClientType: {WAYLAND: 0, X11: 1}},
        Main: {
            sessionMode: {isLocked: false},
            layoutManager: {
                _bgManagers: [{backgroundActor: {_backgroundMenu: menu}}],
                setDummyCursorGeometry: (...args) => opened.push(args),
            },
        },
        global: {
            display: {get_monitor_geometry: () => ({x: 1080, y: 276, width: 2560, height: 1440})},
            backend: {get_monitor_manager: () => ({
                get_monitor_for_connector: connector => (connector === 'DP-3' ? 0 : -1),
            })},
        },
    });
    const extension = vm.runInContext(`${source}\nnew WeLayerdExtension()`, context);
    assert.equal(extension.ShowBackgroundMenu('DP-3', 100, 50), true);
    assert.deepEqual(JSON.parse(JSON.stringify(opened)), [[1180, 326, 0, 0], 'open']);
    assert.equal(extension.ShowBackgroundMenu('HDMI-9', 1, 1), false);
});
