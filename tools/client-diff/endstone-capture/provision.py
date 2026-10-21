"""Provision only: pinned Endstone downloader; never launches BDS or Minecraft."""
import sys
from pathlib import Path

import endstone
import tomlkit
from endstone.cli.windows import WindowsBootstrap

server = Path(sys.argv[1]).resolve()
bootstrap = WindowsBootstrap(str(server), True,
                             'https://raw.githubusercontent.com/EndstoneMC/bedrock-server-data/v2', False)
if endstone.__version__ != '0.11.12' or str(bootstrap.minecraft_version) not in ('26.51', '1.26.51'):
    raise RuntimeError(f'Unexpected Endstone/BDS version: {endstone.__version__}/{bootstrap.minecraft_version}')
# Private APIs are deliberately pinned to Endstone 0.11.12; this performs the same
# download/checksum/configuration stages as Bootstrap.run(), but never calls _run().
bootstrap._install()
bootstrap._validate()
bootstrap._prepare()
(server / 'server.properties').write_text('''server-name=client-diff vanilla reference
server-port=19132
server-portv6=19133
online-mode=false
allow-cheats=true
default-player-permission-level=operator
level-name=client-diff
level-seed=712345
level-type=FLAT
gamemode=creative
force-gamemode=true
difficulty=peaceful
max-players=2
view-distance=4
tick-distance=4
enable-lan-visibility=false
texturepack-required=false
client-side-chunk-generation-enabled=false
compression-threshold=1
compression-algorithm=zlib
content-log-file-enabled=true
''', encoding='utf-8')
config_path = server / 'endstone.toml'
config = tomlkit.parse(config_path.read_text(encoding='utf-8'))
config.setdefault('network', {})['ipv6'] = True
config_path.write_text(tomlkit.dumps(config), encoding='utf-8')
print(f'Provisioned Endstone 0.11.12 / BDS 1.26.51 at {server}; server not started.')
