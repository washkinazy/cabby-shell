#!/usr/bin/python3
"""Private pactl protocol fixture. Copy beside audio-state.json; never contacts audio."""
import json
import os
from pathlib import Path
import sys
import time
root = Path(__file__).parent
args = sys.argv[1:]
with (root / 'audio-pids').open('a') as log:
    log.write(str(os.getpid()) + '\n')
with (root / 'audio-args').open('a') as log:
    log.write(json.dumps(args) + '\n')
if (root / 'audio-offline').exists():
    sys.exit(1)
state_path = root / 'audio-state.json'
def state():
    return json.loads(state_path.read_text())
if args == ['--format=json', 'subscribe']:
    (root / 'audio-subscribed').write_text(str(os.getpid()))
    version = state()['version']
    while True:
        if (root / 'audio-offline').exists():
            sys.exit(1)
        current = state()['version']
        if current != version:
            version = current
            print(json.dumps({'on': 'sink', 'event': 'change'}), flush=True)
        time.sleep(0.05)
elif args == ['--format=json', 'info']:
    print(json.dumps(state()['info']))
elif args[:2] == ['--format=json', 'list']:
    print(json.dumps(state()[args[2]]))
else:
    if (root / 'audio-reject').exists():
        sys.exit(1)
    data = state()
    verb, name = args[:2]
    kind = 'sources' if 'source' in verb else 'sinks'
    device = next(d for d in data[kind] if d['name'] == name)
    if verb.startswith('set-default-'):
        data['info']['default_source_name' if kind == 'sources' else 'default_sink_name'] = name
    elif verb.endswith('-mute'):
        device['mute'] = args[2] == '1'
    elif verb.endswith('-volume'):
        for channel in device['volume'].values():
            channel['value'] = round(int(args[2][:-1]) * 65536 / 100)
    else:
        sys.exit(2)
    data['version'] += 1
    temporary = root / 'audio-state.tmp'
    temporary.write_text(json.dumps(data))
    temporary.replace(state_path)
