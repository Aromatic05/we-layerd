const {readFileSync} = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const {test} = require('node:test');

const source = readFileSync(`${__dirname}/../we-layerd@aromatic/extension.js`, 'utf8')
    .replace(/^import .*;\n/gm, '')
    .replace('export default class WeLayerdExtension', 'class WeLayerdExtension');

function window(options = {}) {
    return {
        get_pid: () => options.pid ?? 10,
        get_title: () => options.title ?? 'App',
        get_wm_class: () => options.wmClass ?? 'app',
        get_window_type: () => options.type ?? 0,
        get_monitor: () => options.monitor ?? 0,
        located_on_workspace: workspace => workspace === 'active' && options.active !== false,
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
        Extension: class {}, Meta: {WindowType: {NORMAL: 0, DESKTOP: 1, DOCK: 2, DIALOG: 3, MODAL_DIALOG: 4}},
        global: {
            display: {
                list_all_windows: () => windows,
                get_focus_window: () => focus,
                get_monitor_geometry: index => (index === 3
                    ? {x: 1920, y: 0, width: 2560, height: 1440}
                    : {x: 0, y: 0, width: 1920, height: 1080}),
            },
            workspace_manager: {get_active_workspace: () => 'active'},
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

test('a borderless window covering its whole monitor counts as fullscreen', () => {
    const borderless = window({monitor: 3, frame: {x: 1920, y: 0, width: 2560, height: 1440}});
    const almost = window({frame: {x: 0, y: 32, width: 1920, height: 1048}});
    assert.deepEqual(states([borderless, almost], borderless), [
        ['HDMI-A-1', true, false, true], ['eDP-1', false, false, false],
    ]);
});

test('no window pauses or mutes the wallpaper while the screen is locked', () => {
    const game = window({fullscreen: true});
    assert.deepEqual(states([game], game, undefined, true), []);
});
