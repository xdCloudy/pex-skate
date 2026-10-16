import os
import re
import shutil
import sys

UPSTREAM_COMMIT = "cb7968930f14dad38457e98720d1a274e469eec2"
HERE = os.path.dirname(os.path.abspath(__file__))


def keep(rel, name):
    ext = os.path.splitext(name)[1].lower()
    if not (ext in (".py", ".json", ".txt", ".md", ".toml", ".rs") or name == "LICENSE"):
        return False
    if rel.startswith("mixamo_to_skate/") and ext == ".json":
        return False
    if re.search(r"(^|/)test_[^/]*\.py$", rel):
        return False
    if re.search(r"(^|/)blender[^/]*(/|$)", rel):
        return False
    return True


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: python import_upstream.py <checkout of SK8-ENGINE/skate-3-rust-engine at %s>" % UPSTREAM_COMMIT[:7])
    src = os.path.join(sys.argv[1], "tools")
    dst = os.path.join(HERE, "tools")
    shutil.rmtree(dst, ignore_errors=True)
    copied = 0
    for base, _, files in os.walk(src):
        if "__pycache__" in base:
            continue
        for name in files:
            full = os.path.join(base, name)
            rel = os.path.relpath(full, src).replace(os.sep, "/")
            if not keep(rel, name):
                continue
            out = os.path.join(dst, rel)
            os.makedirs(os.path.dirname(out), exist_ok=True)
            shutil.copy2(full, out)
            copied += 1
    print("copied %d files into %s" % (copied, dst))


if __name__ == "__main__":
    main()
