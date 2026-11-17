// Generated from the knobdeck-protocol crate (knobdeck-sdk repo). Do not edit: change the Rust
// types and run `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`.

import type { HostMsg, PluginMsg, CompanionMsg, HubMsg, ClientMsg, ServerMsg } from "../src/index.ts";

export const toPlugin: HostMsg[] = [
  {"type":"hello","protocol":2,"plugin_id":"example-rust","data_dir":"/tmp/knobdeck/plugin-data/example-rust","host_version":"1.0.0","features":["state_match","events","notify","app_state","open_dial","companion_hub"],"host_pid":4242},
  {"type":"sync","instances":[{"instance_id":"i1","action_id":"example-rust-mute-0a1b2c3d","inputs":{}}]},
  {"type":"invoke","id":"r1","instance_id":"i1","action_id":"example-rust-mute-0a1b2c3d","trigger":"press","inputs":{}},
  {"type":"invoke","id":"r2","instance_id":"v1","action_id":"example-rust-volume-4e5f6a7b","trigger":"adjust","inputs":{"value":40}},
  {"type":"invoke","id":"r3","instance_id":"w1::3","action_id":"example-rust-recent-8c9d0e1f","trigger":"press","inputs":{},"child_id":"3"},
  {"type":"request","id":"q1","method":"query","params":{"query_id":"items","inputs":{"group":"fruit"}}},
  {"type":"event","event":"focus_changed","payload":{"app":"code.exe"}},
  {"type":"result","id":"h1","ok":true,"value":null},
  {"type":"result","id":"h2","ok":false,"error":"not possible"},
  {"type":"shutdown"},
  {"type":"event","event":"dial_opened","payload":{"profile_id":"default"}},
  {"type":"event","event":"dial_closed","payload":{}},
  {"type":"result","id":"c2","ok":true,"value":{"os":"linux","host_version":"1.0.0","dial_open":false}},
  {"type":"event","event":"companion_connected","payload":{"link":"photoshop-1","instance":"6f1c2b9e-ps-2026","app":"photoshop","app_version":"27.1.0","name":"Photoshop 2026","version":"1.0.0","features":["layers"]}},
  {"type":"event","event":"companion_event","payload":{"link":"photoshop-1","event":"tool_changed","payload":{"tool":"eraser"}}},
  {"type":"event","event":"companion_disconnected","payload":{"link":"photoshop-1"}},
  {"type":"result","id":"c5","ok":true,"value":{"id":"7","ok":true,"result":{"tool":"brush"}}},
];

export const toHost: PluginMsg[] = [
  {"type":"ready","protocol":2,"subscribe":["focus_changed"],"done":true},
  {"type":"ack","id":"r1"},
  {"type":"done","id":"r1"},
  {"type":"state","instance_id":"i1","state":"muted"},
  {"type":"state","action_id":"example-rust-mute-0a1b2c3d","state":"muted","label":"Muted","icon":"mic-off"},
  {"type":"state","action_id":"example-rust-device-volume-2b3c4d5e","match":{"device":"a"},"state":"40"},
  {"type":"error","instance_id":"i1","message":"demo error"},
  {"type":"error","action_id":"example-rust-mute-0a1b2c3d","match":{"device":"a"},"id":"r1","message":"demo error"},
  {"type":"result","id":"q1","ok":true,"value":["apple","pear"]},
  {"type":"result","id":"q2","ok":false,"error":"unknown query \"nope\""},
  {"type":"host_call","id":"c1","method":"send_keys","params":{"keys":"ctrl+m"}},
  {"type":"subscribe","events":["focus_changed"]},
  {"type":"setup_required","reason":"demo token expired"},
  {"type":"host_call","id":"c2","method":"notify","params":{"message":"Copied","level":"info"}},
  {"type":"host_call","id":"c3","method":"app_state","params":{}},
  {"type":"subscribe","events":["dial_opened","dial_closed","profiles_changed"]},
  {"type":"host_call","id":"c4","method":"open_dial","params":{"profile_id":"streaming"}},
  {"type":"host_call","id":"c5","method":"companion_request","params":{"link":"photoshop-1","action":"set-tool","params":{"tool":"brush"},"timeout_ms":5000}},
  {"type":"host_call","id":"c6","method":"companion_links","params":{}},
];

export const toHub: CompanionMsg[] = [
  {"type":"hello","protocol":1,"plugin":"photoshop","instance":"6f1c2b9e-ps-2026","app":"photoshop","app_version":"27.1.0","name":"Photoshop 2026","version":"1.0.0","features":["layers"],"token":"3b6e…"},
  {"type":"hello","protocol":1,"plugin":"browser-plugin","instance":"a4d0-chrome-work","app":"chrome","features":[]},
  {"type":"ping"},
  {"type":"response","id":"7","ok":true,"result":{"tool":"brush"}},
  {"type":"response","id":"8","ok":false,"error":"No document is open"},
  {"type":"response","id":"9","ok":false,"unsupported":true},
  {"type":"event","event":"tool_changed","payload":{"tool":"eraser"}},
];

export const toCompanion: HubMsg[] = [
  {"type":"ready","protocol":1,"link":"photoshop-1","host_version":"1.0.0"},
  {"type":"refused","reason":"wrong token"},
  {"type":"pong"},
  {"type":"request","id":"7","action":"set-tool","params":{"tool":"brush"}},
];

export const toServer: ClientMsg[] = [
  {"type":"hello","protocol":1,"token":"9f2c…","client":"obs-script"},
  {"type":"hello","protocol":1,"token":"9f2c…","client":"knobdeck-mcp","assistant":true},
  {"type":"request","id":"1","cmd":"open_dial","params":{"profile_id":"streaming"}},
  {"type":"request","id":"2","cmd":"run_item","params":{"profile_id":"streaming","item_id":"mute-mic"}},
  {"type":"request","id":"3","cmd":"run_item","params":{"profile_id":"streaming","item_id":"volume","value":40.0}},
  {"type":"request","id":"4","cmd":"list_profiles","params":{}},
  {"type":"request","id":"5","cmd":"subscribe","params":{"events":["dial_opened","dial_closed"]}},
  {"type":"request","id":"6","cmd":"close_dial","params":{}},
  {"type":"request","id":"7","cmd":"list_plugins","params":{}},
  {"type":"request","id":"8","cmd":"list_items","params":{"profile_id":"streaming"}},
];

export const toClient: ServerMsg[] = [
  {"type":"ready","protocol":1,"host_version":"1.0.0","commands":["open_dial","close_dial","run_item","list_profiles","list_plugins","subscribe","list_items"]},
  {"type":"refused","reason":"wrong token"},
  {"type":"response","id":"1","ok":true,"result":true},
  {"type":"response","id":"4","ok":true,"result":[{"id":"default","name":"Default","default":true},{"id":"streaming","name":"Streaming","default":false,"assistant":true}]},
  {"type":"response","id":"7","ok":true,"result":[{"id":"obs","name":"OBS Studio","version":"1.4.0","status":"running"}]},
  {"type":"response","id":"8","ok":true,"result":[{"id":"mute-mic","label":"Mute Mic","kind":"toggle","plugin":"discord"},{"id":"volume","label":"Volume","kind":"slider","min":0.0,"max":100.0,"unit":"%"},{"id":"scene-live","label":"Live","kind":"button","plugin":"obs","folder":"Scenes"}]},
  {"type":"response","id":"2","ok":false,"error":"no item \"mute-mic\" in dial profile \"streaming\""},
  {"type":"event","event":"dial_opened","payload":{"profile_id":"streaming"}},
];
