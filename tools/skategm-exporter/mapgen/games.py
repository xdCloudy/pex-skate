from dataclasses import dataclass, field


@dataclass(frozen=True)
class Stream:
    archive: str
    name: str


@dataclass(frozen=True)
class Game:
    id: str
    title: str
    prefix: str
    marker: str
    db: str
    misc: str
    dmo: str = ''
    world: str = 'data/content/world{district}.big'
    proxies: dict = field(default_factory=dict, hash=False, compare=False)
    proxy_archive: str = 'data/content/proxy{stream}.big'
    streams: dict = field(default_factory=dict, hash=False, compare=False)
    decoder: str = 'prepare_university'
    decode_options: dict = field(default_factory=dict, hash=False, compare=False)
    sky: str = 'vlt'
    # its db.big holds the VLT world rows (each district's environment: sky,
    # fog, sun, sea model); Skate 2's doesn't
    world_db: bool = True
    sky_archive: str = ''
    sky_textures: str = ''
    sky_layers: tuple = ()
    lightmap_channels: bool = False

    def stream(self, district):
        if district in self.streams:
            return Stream(*self.streams[district])
        return Stream(self.world.format(district=district), district)

    def proxy(self, district):
        stream = self.proxies.get(district)
        if stream is None:
            return None
        if isinstance(stream, tuple):
            return Stream(*stream)
        return Stream(self.proxy_archive.format(stream=stream), stream)


SKATE3 = Game(
    id='skate3', title='Skate 3', prefix='sgm_skate3_', marker='data/big/db.big',
    db='data/big/db.big', misc='data/big/miscload.big', dmo='data/content/worlddmo.big',
    proxies={'DIST_University': 'University_100_Proxy', 'DIST_DownTown': 'Downtown_100_Proxy',
             'DIST_Industrial': 'Industrial_100_Proxy'},
)

SKATE2 = Game(
    id='skate2', title='Skate 2', prefix='sgm_skate2_', marker='data/content/worldbam.big',
    db='data/db/db.big', misc='data/big/miscload.big',
    streams={'BAM': ('data/content/worldbam.big', 'BAM')},
    proxies={'BAM': ('data/content/worldproxy.big', 'PROXY')},
    decoder='prepare_skate2_bam',
    decode_options={'allow_material_import_order_fallback': True, 'grind_coordinate_mode': 'mixed_cell_local'},
    sky='dome', sky_archive='data/big/miscboot.big', sky_textures='data/content/world/models/DIST_skybox_Textures.rx2',
    sky_layers=(0, 3), lightmap_channels=True, world_db=False,
)

GAMES = {g.id: g for g in (SKATE3, SKATE2)}


def get(game_id):
    if isinstance(game_id, Game):
        return game_id
    try:
        return GAMES[game_id]
    except KeyError:
        raise KeyError(f'no game called {game_id}') from None


def detect(names):
    have = {n.replace('\\', '/').lower() for n in names}
    for game in (SKATE2, SKATE3):
        if game.marker.lower() in have:
            return game
    return None
