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
    };
}

function states(windows, focus = windows[0], target = {pid: 50, title: 'Renderer', wmClass: 'renderer'}) {
    const context = vm.createContext({
        Extension: class {}, Meta: {WindowType: {NORMAL: 0, DIALOG: 2, MODAL_DIALOG: 3}},
        global: {
            display: {list_all_windows: () => windows, get_focus_window: () => focus},
            workspace_manager: {get_active_workspace: () => 'active'},
            backend: {get_monitor_manager: () => ({get_logical_monitors: () => [
                {get_number: () => 3, get_monitors: () => [{get_connector: () => 'HDMI-A-1'}]},
                {get_number: () => 0, get_monitors: () => [{get_connector: () => 'eDP-1'}]},
            ]})},
        },
    });
    const extension = vm.runInContext(`${source}\nnew WeLayerdExtension()`, context);
    extension._target = target;
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
    assert.deepEqual(states([normal, sticky, hidden, minimized, renderer, window({type: 1})], normal), [
        ['eDP-1', true, false, false], ['eDP-1', false, false, false],
    ]);
});

test('unregistered renderer does not publish rule activity', () => {
    assert.deepEqual(states([window()], null, null), []);
});

test('focus on application dialogs remains active for focused-window rules', () => {
    for (const type of [2, 3]) {
        const parent = window();
        const dialog = window({pid: 11, type});
        assert.deepEqual(states([parent, dialog], dialog), [
            ['eDP-1', false, false, false], ['eDP-1', true, false, false],
        ]);
    }
});
