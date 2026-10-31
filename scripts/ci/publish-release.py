#!/usr/bin/env python3
"""Attach release archives to the GitHub release for an existing tag.

The release workflow's publish step, through the REST API with the job
token and nothing else: no third-party action and no `gh` (the self-hosted
runner's PATH may hold a wrapper meant for interactive use).

    GITHUB_TOKEN=... publish-release.py --repo owner/name --tag v1.2.3 FILE...
    GITHUB_TOKEN=... publish-release.py --repo owner/name --tag v1.2.3 --check-tag

The tag must already exist: the release is never allowed to mint one. The
release is created when the tag has none (a prerelease when the tag carries
a `-`), and an asset already on it under the same name is replaced.
"""

import argparse
import json
import mimetypes
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.github.com"


def request(method, url, token, body=None, data=None, content_type=None):
    headers = {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    if body is not None:
        data = json.dumps(body).encode()
        content_type = "application/json"
    if content_type:
        headers["Content-Type"] = content_type
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            raw = resp.read()
            return resp.status, json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return 404, None
        sys.exit(f"ERROR: {method} {url}: HTTP {e.code} {e.read()[:500]!r}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", required=True)
    ap.add_argument("--tag", required=True)
    ap.add_argument("--check-tag", action="store_true", help="only check the tag exists")
    ap.add_argument("files", nargs="*")
    args = ap.parse_args()
    token = os.environ.get("GITHUB_TOKEN")
    if not token:
        sys.exit("ERROR: GITHUB_TOKEN is not set")
    repo = f"{API}/repos/{args.repo}"
    tag = urllib.parse.quote(args.tag, safe="")

    status, _ = request("GET", f"{repo}/git/ref/tags/{tag}", token)
    if status == 404:
        sys.exit(f"ERROR: '{args.tag}' is not an existing tag; refusing to publish")
    if args.check_tag:
        print(f"tag {args.tag} exists")
        return
    if not args.files:
        sys.exit("ERROR: no files to attach")
    for f in args.files:
        if not os.path.isfile(f):
            sys.exit(f"ERROR: {f} is not a file")

    status, release = request("GET", f"{repo}/releases/tags/{tag}", token)
    if status == 404:
        _, release = request(
            "POST",
            f"{repo}/releases",
            token,
            body={
                "tag_name": args.tag,
                "name": args.tag,
                "draft": False,
                "prerelease": "-" in args.tag,
            },
        )
        print(f"created release {args.tag}")
    existing = {a["name"]: a["id"] for a in release.get("assets", [])}
    upload = release["upload_url"].split("{", 1)[0]
    for f in args.files:
        name = os.path.basename(f)
        if name in existing:
            request("DELETE", f"{repo}/releases/assets/{existing[name]}", token)
        kind = mimetypes.guess_type(name)[0] or "application/octet-stream"
        with open(f, "rb") as fh:
            data = fh.read()
        request(
            "POST",
            f"{upload}?name={urllib.parse.quote(name)}",
            token,
            data=data,
            content_type=kind,
        )
        print(f"attached {name} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
