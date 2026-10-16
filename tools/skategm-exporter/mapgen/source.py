import sys
from pathlib import Path

from . import EXPORTER
from . import games


def _xiso():
    if str(EXPORTER) not in sys.path:
        sys.path.insert(0, str(EXPORTER))
    import xiso
    return xiso


class GameSource:
    def __init__(self, path, cache, game=None):
        path = Path(path)
        if path.is_file() and path.name.lower() == 'default.xex':
            path = path.parent
        self.path = path
        self.cache = Path(cache)
        self.iso = path.is_file() and path.suffix.lower() == '.iso'
        if not self.iso and not (path / 'data').is_dir():
            raise FileNotFoundError(f'{path} is neither a game disc image nor a folder holding data/')
        self.game = games.get(game) if game else self.detect()
        self.cache = self.cache / self.game.id

    def scope(self, folder):
        return Path(folder) / self.game.id

    def detect(self):
        if self.iso:
            with _xiso().Xiso(self.path) as disc:
                found = games.detect(p for p, _ in disc.walk())
        else:
            found = next((g for g in (games.SKATE2, games.SKATE3) if (self.path / g.marker).is_file()), None)
        return found or games.SKATE3

    def file(self, relative, report=print):
        relative = relative.replace('\\', '/')
        if not self.iso:
            found = self.path / relative
            if not found.is_file():
                raise FileNotFoundError(f'{relative} not found under {self.path}')
            return found
        target = self.cache / relative
        if target.is_file():
            return target
        partial = target.with_name(target.name + '.part')
        partial.parent.mkdir(parents=True, exist_ok=True)
        with _xiso().Xiso(self.path) as disc:
            disc.extract(relative, str(partial))
        partial.replace(target)
        return target

    def district(self, district, report=print):
        return self.file(self.game.stream(district).archive, report)
