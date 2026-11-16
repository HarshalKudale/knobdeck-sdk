// Generated from the knobdeck-protocol crate (knobdeck-sdk repo). Do not edit: change the Rust
// types and run `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`.

import type { HostMsg, PluginMsg } from "./protocol";

export const toPlugin: HostMsg[] = [
  {"type":"hello","protocol":2,"plugin_id":"example-rust","data_dir":"/tmp/knobdeck/plugin-data/example-rust","host_version":"1.0.0","features":["state_match","events","notify","app_state","open_dial"],"host_pid":4242},
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
];
