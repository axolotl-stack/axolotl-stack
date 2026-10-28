"""Run the installed Endstone entity probe through TOOLING's BDS lifecycle helper.

Requires exclusive BDS ownership and an already-built sweep or capture client;
never launches Minecraft or builds while BDS is active. Retains raw tick data.
"""

import argparse
import importlib
import json
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--harness', type=Path, required=True)
    parser.add_argument('--client', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--probe',
                        choices=['motion', 'fluid-contact', 'kinematics', 'flow-corners',
                                 'ground-liquid', 'player-drop'],
                        default='motion')
    parser.add_argument('--wire-capture', action='store_true',
                        help='client is bds-extractor; wait for its packet recorder to flush')
    parser.add_argument('--sweep-capture', action='store_true',
                        help='client is bds-sweep-client with raw packet recording')
    parser.add_argument('--drop-angle-controls', action='store_true',
                        help='player-drop: capture extra fractional/wrapped angle controls')
    parser.add_argument('--capture-seconds', type=int)
    args = parser.parse_args()
    if args.wire_capture and args.sweep_capture:
        parser.error('choose one capture-client protocol')
    if args.probe == 'player-drop' and not args.sweep_capture:
        parser.error('player-drop requires bds-sweep-client and --sweep-capture')
    sys.path.insert(0, str(args.harness.resolve()))
    runtime = importlib.import_module('bds_runtime')
    client_path = args.client.resolve()
    if not client_path.is_file():
        parser.error(f'client binary missing: {client_path}')
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    level = 'entity_motion_probe_' + uuid.uuid4().hex
    world = runtime.SERVER / 'worlds' / level
    properties = runtime.base_properties(
        level, level_type='FLAT', gamemode='creative', force_gamemode='true',
        difficulty='peaceful', default_player_permission_level='operator',
        view_distance=6, tick_distance=4)
    env = {'ENTITY_MOTION_PROBE_OUT': str(out), 'ENTITY_MOTION_PROBE_MODE': args.probe,
           'BDS_SWEEP_JOB': '', 'CLIENT_DIFF_CAPTURE_DIR': str(out / 'endstone-wire')}
    if args.drop_angle_controls:
        env['ENTITY_MOTION_PROBE_ANGLE_CONTROLS'] = '1'
    capture_seconds = args.capture_seconds or (60 if args.probe == 'motion' else 25)
    client_args = [str(client_path), '--name', 'BDSSweep']
    if args.wire_capture:
        client_args.extend(['--capture-dir', str(out / 'wire'), '--settle', str(capture_seconds),
                            '--timeout', '30'])
    if args.sweep_capture:
        client_args.extend(['--capture-dir', str(out / 'wire')])
    started = time.monotonic()
    try:
        with runtime.Bds(properties, out / 'bds.log', endstone=True, env=env):
            with (out / 'client.log').open('w', encoding='utf-8') as log:
                client = subprocess.Popen(client_args, stdout=log, stderr=subprocess.STDOUT)
                observer = None
                observer_log = None
                try:
                    if args.probe == 'player-drop':
                        # Native RakNet refused simultaneous loopback logins
                        # with "ip recently connected"; stagger startup.
                        time.sleep(2)
                        observer_log = (out / 'observer-client.log').open('w', encoding='utf-8')
                        observer = subprocess.Popen(
                            [str(client_path), '--name', 'BDSOther',
                             '--capture-dir', str(out / 'observer-wire')],
                            stdout=observer_log, stderr=subprocess.STDOUT)
                    while time.monotonic() - started < 240:
                        if (out / 'error.txt').exists():
                            raise RuntimeError((out / 'error.txt').read_text(encoding='utf-8'))
                        if (out / 'done.json').exists():
                            print((out / 'done.json').read_text(encoding='utf-8'))
                            if args.wire_capture:
                                code = client.wait(timeout=capture_seconds + 45)
                                if code != 0:
                                    raise RuntimeError(f'capture client exited {code}; see {out / "client.log"}')
                            if args.sweep_capture:
                                for process in (client, observer):
                                    if process is not None and process.wait(timeout=15) != 0:
                                        raise RuntimeError('scripted capture client failed; see client logs')
                            return 0
                        if client.poll() is not None:
                            raise RuntimeError(f'client exited {client.returncode}; see {out / "client.log"}')
                        if observer is not None and observer.poll() is not None:
                            raise RuntimeError(f'observer exited {observer.returncode}; see observer-client.log')
                        time.sleep(0.25)
                    raise TimeoutError(f'probe did not finish within 240s; see {out}')
                finally:
                    for process in (client, observer):
                        if process is not None:
                            if process.poll() is None:
                                process.kill()
                            process.wait(timeout=10)
                    if observer_log is not None:
                        observer_log.close()
    finally:
        if world.exists():
            shutil.rmtree(world)


if __name__ == '__main__':
    sys.exit(main())
