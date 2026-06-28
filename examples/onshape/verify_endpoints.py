#!/usr/bin/env python3
"""Spec-conformance test: every REST endpoint cited in the onshape jd library must
exist in the real Onshape OpenAPI spec (captures/openapi.json, the documented
ground truth). Exercises the documented surface without needing live credentials.

Run:  python3 verify_endpoints.py            # uses the cached spec
      python3 verify_endpoints.py --refresh  # re-download the spec first
"""
import json, os, re, sys, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = os.path.join(HERE, "captures", "openapi.json")
JD_DIR = os.path.normpath(os.path.join(HERE, "..", "..", ".jd", "library", "dev", "onshape"))
SPEC_URL = "https://cad.onshape.com/api/openapi"

HEX24 = re.compile(r"^[0-9a-f]{16,32}$")
# path-like tokens in the jd prose/recipes (optionally version-pinned)
PATH_RE = re.compile(r"/(?:api/v\d+/)?(?:" + "|".join([
    "documents", "partstudios", "assemblies", "parts", "elements", "appelements",
    "blobelements", "metadata", "metadatacategory", "variables", "drawings",
    "translations", "thumbnails", "webhooks", "users", "teams", "companies",
    "accounts", "featurestudios", "folders", "revisions", "exportrules",
]) + r")[A-Za-z0-9_{}/\-]*")


def norm(path):
    """Normalize a path to a comparable template: strip /api/vN, drop query,
    collapse {param}/ids/w|v|m-coordinate selectors to '*'."""
    path = path.split("?")[0].split("#")[0].rstrip("/.,)`'\"")
    path = re.sub(r"^/api/v\d+", "", path)
    out = []
    for seg in path.split("/"):
        if not seg:
            out.append(seg)
            continue
        if seg.startswith("{") or HEX24.match(seg) or seg in ("w", "v", "m", "wvm", "wvmid"):
            out.append("*")
        else:
            out.append(seg)
    return "/".join(out)


def spec_templates():
    spec = json.load(open(SPEC))
    return {norm(p) for p in spec["paths"]}


def conformant(cited, templates):
    """A cited path conforms if a spec template matches it segment-aligned, allowing
    either side to be a prefix of the other ('*' matches any one segment). This
    tolerates partial citations (a truncated fragment, a concrete id suffix) while
    still rejecting any endpoint whose path shape exists in no spec template."""
    cs = cited.split("/")
    for t in templates:
        ts = t.split("/")
        n = min(len(cs), len(ts))
        if n <= 1:
            continue
        if all(a == b or a == "*" or b == "*" for a, b in zip(cs[:n], ts[:n])):
            return True
    return False


def cited_paths():
    cites = {}  # normalized -> (file, raw)
    for fn in sorted(os.listdir(JD_DIR)):
        if not fn.endswith(".jd"):
            continue
        text = open(os.path.join(JD_DIR, fn)).read()
        for m in PATH_RE.finditer(text):
            raw = m.group(0)
            n = norm(raw)
            segs = [s for s in n.split("/") if s]
            if len(segs) < 2:                       # bare group or group-only mention
                continue
            if segs[-1] in ("d", "w", "v", "m"):    # truncated fragment, not an endpoint
                continue
            if segs[0] == "documents" and segs[1] != "d":  # web-app URL, not the REST path
                continue
            cites.setdefault(n, (fn, raw))
    return cites


def main():
    if "--refresh" in sys.argv or not os.path.exists(SPEC):
        os.makedirs(os.path.dirname(SPEC), exist_ok=True)
        print(f"[spec] fetching {SPEC_URL} (public, no auth needed) …")
        urllib.request.urlretrieve(SPEC_URL, SPEC)
        print(f"[spec] saved {SPEC} ({os.path.getsize(SPEC)} bytes)")
    templates = spec_templates()
    cites = cited_paths()
    matched, unmatched = [], []
    for n, (fn, raw) in sorted(cites.items()):
        (matched if conformant(n, templates) else unmatched).append((n, fn, raw))
    print(f"spec path templates: {len(templates)}")
    print(f"distinct endpoints cited across onshape jds: {len(cites)}")
    print(f"matched: {len(matched)}   unmatched: {len(unmatched)}")
    if unmatched:
        print("\nUNMATCHED (cited in a jd but not found in the OpenAPI spec):")
        for n, fn, raw in unmatched:
            print(f"  {fn}: {raw}   (normalized {n})")
        print("\nFAIL — every cited endpoint must exist in the spec.")
        return 1
    print("\nPASS — every endpoint cited in the onshape jd library exists in the real OpenAPI spec.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
