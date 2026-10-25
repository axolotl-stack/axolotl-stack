"""Copy the committed subset of a T1 capture into the repository.

    python tools/bds-sweep/commit_fixtures.py [CAPTURE_DIR]

CAPTURE_DIR defaults to the one named by `<BDS runtime>/captures/1.26.51/
latest.txt`. Captures are Mojang-derived (decision D13): only manifests and
small packet fixtures that tests use are committed, into
`crates/bds-extractor/captures/1.26.51/`:

* both sessions' `manifest.json` (SHA-256 of every file, committed or not),
  `session.json` and the member `join-sequence.json`;
* member join packets up to `PlayStatus(PlayerSpawn)`, except world payload
  and per-tick actor/world updates, at most `MAX_BYTES` each;
* every operator `summons/*.bin` (one AddActor per summonable identifier).

Everything else (CraftingData, AvailableCommands, JigsawStructureData, the
full `packets.jsonl`, normalised JSON) stays in the capture directory; tests
that need it read it from there and check the manifest hash.
"""

from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
DEST = REPO / 'crates' / 'bds-extractor' / 'captures' / '1.26.51'
MAX_BYTES = 128 * 1024
# World payload and per-tick world/actor traffic: depends on the terrain and
# nearby mobs, not on the join contract.
SKIP = {
    'LevelChunkPacket', 'SubChunkPacket', 'NetworkChunkPublisherUpdatePacket',
    'UpdateSubChunkBlocksPacket', 'UpdateBlockPacket', 'LevelSoundEventPacket',
    'CurrentStructureFeaturePacket', 'MoveActorDeltaPacket', 'MoveActorAbsolutePacket',
    'SetActorMotionPacket', 'AddActorPacket', 'RemoveActorPacket', 'MobEquipmentPacket',
    'MobArmorEquipmentPacket', 'ActorEventPacket', 'AddItemActorPacket',
}


def main() -> int:
    if len(sys.argv) > 1:
        capture = Path(sys.argv[1]).resolve()
    else:
        capture = Path((bds_runtime.CAPTURES / 'latest.txt').read_text(encoding='utf-8').strip())
    if DEST.exists():
        shutil.rmtree(DEST)
    committed = []

    def copy(relative: str) -> None:
        target = DEST / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(capture / relative, target)
        committed.append(relative)

    for session in ('member', 'operator'):
        copy(f'{session}/manifest.json')
        copy(f'{session}/session.json')
    copy('member/join-sequence.json')

    sequence = json.loads((capture / 'member' / 'join-sequence.json').read_text(encoding='utf-8'))
    start_game = next(row['seq'] for row in sequence if row['packet'] == 'StartGamePacket')
    spawn = next(row['seq'] for row in sequence if row['packet'] == 'PlayStatusPacket' and row['seq'] > start_game)
    for row in sequence:
        if row['seq'] > spawn or row['packet'] in SKIP or row['bytes'] > MAX_BYTES:
            continue
        copy(f'member/join/{row["seq"]:05d}-{row["packet"]}.bin')
    for path in sorted((capture / 'operator' / 'summons').glob('*.bin')):
        copy(f'operator/summons/{path.name}')

    total = sum((DEST / relative).stat().st_size for relative in committed)
    print(f'committed {len(committed)} files, {total} bytes, from {capture}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
