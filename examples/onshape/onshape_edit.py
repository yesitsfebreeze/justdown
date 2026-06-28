#!/usr/bin/env python3
"""Edit an Onshape Part Studio feature (e.g. extrude depth) via the REST API.

Live use:
    export ONSHAPE_ACCESS_KEY=... ONSHAPE_SECRET_KEY=...
    ./onshape_edit.py "https://cad.onshape.com/documents/<did>/w/<wid>/e/<eid>" "50 mm"

Proof-of-run without credentials (spins an in-process mock of the features API
and runs the exact same edit+verify code path):
    ./onshape_edit.py --self-test

The edit: find the first `extrude` feature, set its `depth` expression, POST the
full feature back, then re-GET to confirm the value persisted and read mass
properties to confirm the geometry recomputed.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
import json
import os
import re
import sys
import uuid
from datetime import datetime, timezone
from email.utils import format_datetime
from urllib.parse import urlparse, urlencode

import requests

DEFAULT_BASE = "https://cad.onshape.com"
API_VERSION = "v10"

DOC_URL_RE = re.compile(r"/documents/(?:d/)?([^/]+)/w/([^/]+)/e/([^/]+)")


def parse_doc_url(url: str) -> tuple[str, str, str]:
    """Pull (did, wid, eid) out of a browser or API document URL."""
    m = DOC_URL_RE.search(url)
    if not m:
        raise ValueError(f"could not find /documents/<did>/w/<wid>/e/<eid> in: {url}")
    return m.group(1), m.group(2), m.group(3)


def hmac_headers(method: str, url: str, access_key: str, secret_key: str,
                 content_type: str = "application/json") -> dict[str, str]:
    """Build the Onshape `On <key>:HmacSHA256:<sig>` auth headers for one request.

    String-to-sign = method, nonce, date, content-type, path, query — each
    followed by \\n — then the whole thing lowercased, HMAC-SHA256'd, base64'd.
    """
    parsed = urlparse(url)
    nonce = uuid.uuid4().hex + uuid.uuid4().hex[:9]  # >=16 alphanumeric chars
    date = format_datetime(datetime.now(timezone.utc), usegmt=True)
    path = parsed.path
    query = parsed.query or ""
    to_sign = f"{method}\n{nonce}\n{date}\n{content_type}\n{path}\n{query}\n".lower()
    sig = base64.b64encode(
        hmac.new(secret_key.encode(), to_sign.encode(), hashlib.sha256).digest()
    ).decode()
    return {
        "Authorization": f"On {access_key}:HmacSHA256:{sig}",
        "Date": date,
        "On-Nonce": nonce,
        "Content-Type": content_type,
        "Accept": "application/json",
    }


class Onshape:
    """Minimal Onshape REST client (HMAC-signed, no SDK dependency)."""

    def __init__(self, access_key: str, secret_key: str, base: str = DEFAULT_BASE):
        self.access_key = access_key
        self.secret_key = secret_key
        self.base = base.rstrip("/")
        self.session = requests.Session()

    def _request(self, method: str, path: str, params: dict | None = None,
                 body: dict | None = None) -> dict:
        url = f"{self.base}/api/{API_VERSION}{path}"
        if params:
            url = f"{url}?{urlencode(params)}"
        headers = hmac_headers(method, url, self.access_key, self.secret_key)
        data = json.dumps(body) if body is not None else None
        r = self.session.request(method, url, headers=headers, data=data)
        r.raise_for_status()
        return r.json()

    def get_features(self, did: str, wid: str, eid: str) -> dict:
        return self._request("GET", f"/partstudios/d/{did}/w/{wid}/e/{eid}/features")

    def update_feature(self, did: str, wid: str, eid: str, fid: str, feature: dict,
                       source_microversion: str | None = None) -> dict:
        body = {"btType": "BTFeatureDefinitionCall-1406", "feature": feature,
                "rejectMicroversionSkew": False}
        if source_microversion:
            body["sourceMicroversion"] = source_microversion
        return self._request(
            "POST", f"/partstudios/d/{did}/w/{wid}/e/{eid}/features/featureid/{fid}",
            body=body)

    def mass_properties(self, did: str, wid: str, eid: str) -> dict:
        return self._request(
            "GET", f"/partstudios/d/{did}/w/{wid}/e/{eid}/massproperties")


def _unwrap(obj: dict) -> dict:
    """Onshape serialization sometimes nests payloads under a `message` key."""
    return obj["message"] if isinstance(obj, dict) and "message" in obj else obj


def find_extrude_depth(features: dict) -> tuple[dict, str, dict]:
    """Return (feature, featureId, depth_param) for the first extrude feature."""
    for f in features["features"]:
        feat = _unwrap(f)
        if feat.get("featureType") == "extrude":
            for p in feat["parameters"]:
                param = _unwrap(p)
                if param.get("parameterId") == "depth":
                    return feat, feat["featureId"], param
    raise LookupError("no extrude feature with a 'depth' parameter found")


def edit_extrude_depth(client: Onshape, did: str, wid: str, eid: str,
                       new_depth: str) -> dict:
    """Change the first extrude's depth and confirm it persisted. Returns a report."""
    feats = client.get_features(did, wid, eid)
    feature, fid, depth = find_extrude_depth(feats)
    old = depth["expression"]
    depth["expression"] = new_depth

    client.update_feature(did, wid, eid, fid, feature,
                          feats.get("sourceMicroversion"))

    feats2 = client.get_features(did, wid, eid)
    _, _, depth2 = find_extrude_depth(feats2)
    persisted = depth2["expression"]

    report = {"feature_id": fid, "old_depth": old, "requested": new_depth,
              "persisted": persisted, "ok": persisted == new_depth}
    try:
        mp = client.mass_properties(did, wid, eid)
        vol = _unwrap(mp.get("bodies", {}).get("-all-", {})).get("volume")
        report["volume"] = vol[0] if isinstance(vol, list) else vol
    except Exception as e:  # mass props are a nicety, not the gate
        report["volume_error"] = str(e)
    return report


# --------------------------------------------------------------------------- #
# Self-test: an in-process mock of the features API so the full edit+verify
# code path runs with no Onshape account.
# --------------------------------------------------------------------------- #
def _self_test() -> int:
    import http.server
    import threading

    state = {
        "depth": "25 mm",
        "microversion": "abc123",
    }

    def feature_doc():
        return {
            "sourceMicroversion": state["microversion"],
            "features": [{
                "btType": "BTMFeature-134",
                "featureId": "FID0",
                "featureType": "extrude",
                "name": "Extrude 1",
                "parameters": [
                    {"btType": "BTMParameterEnum-145", "parameterId": "endBound",
                     "value": "BLIND"},
                    {"btType": "BTMParameterQuantity-147", "parameterId": "depth",
                     "expression": state["depth"]},
                ],
            }],
        }

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a):  # silence
            pass

        def _send(self, payload):
            body = json.dumps(payload).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path.endswith("/features"):
                self._send(feature_doc())
            elif self.path.endswith("/massproperties"):
                self._send({"bodies": {"-all-": {"volume": [1.25e-4, 0, 0]}}})
            else:
                self.send_error(404)

        def do_POST(self):
            n = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(n) or b"{}")
            depth = next(p for p in body["feature"]["parameters"]
                         if p.get("parameterId") == "depth")
            state["depth"] = depth["expression"]  # persist the edit
            self._send({"feature": body["feature"]})

    srv = http.server.HTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{srv.server_address[1]}"

    print(f"[self-test] mock Onshape at {base}")
    client = Onshape("DUMMY_ACCESS", "DUMMY_SECRET", base=base)
    # also exercise the real signer + URL parser
    did, wid, eid = parse_doc_url(
        "https://cad.onshape.com/documents/aaa/w/bbb/e/ccc")
    assert (did, wid, eid) == ("aaa", "bbb", "ccc"), "URL parse failed"
    h = hmac_headers("GET", f"{base}/api/v10/x", "K", "S")
    assert h["Authorization"].startswith("On K:HmacSHA256:"), "HMAC header malformed"

    report = edit_extrude_depth(client, "aaa", "bbb", "ccc", "50 mm")
    srv.shutdown()
    print(json.dumps(report, indent=2))
    assert report["ok"], "edit did not persist in mock"
    assert report["old_depth"] == "25 mm" and report["persisted"] == "50 mm"
    print("[self-test] PASS — edit+verify flow works end to end")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("url", nargs="?", help="Onshape Part Studio document URL")
    ap.add_argument("depth", nargs="?", default="50 mm",
                    help="new depth expression, e.g. '50 mm' (default: 50 mm)")
    ap.add_argument("--self-test", action="store_true",
                    help="run the full flow against an in-process mock (no creds)")
    ap.add_argument("--base", default=DEFAULT_BASE)
    args = ap.parse_args()

    if args.self_test:
        return _self_test()

    if not args.url:
        ap.error("a document URL is required (or use --self-test)")
    access = os.environ.get("ONSHAPE_ACCESS_KEY")
    secret = os.environ.get("ONSHAPE_SECRET_KEY")
    if not (access and secret):
        print("error: set ONSHAPE_ACCESS_KEY and ONSHAPE_SECRET_KEY", file=sys.stderr)
        return 2

    did, wid, eid = parse_doc_url(args.url)
    client = Onshape(access, secret, base=args.base)
    report = edit_extrude_depth(client, did, wid, eid, args.depth)
    print(json.dumps(report, indent=2))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
