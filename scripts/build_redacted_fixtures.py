#!/usr/bin/env python3
"""Build redacted-real Plan 004 fixtures from approved local exports."""

from __future__ import annotations

import csv
import hashlib
import io
import json
import re
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
FIXTURES = REPO / "backend" / "tests" / "fixtures"
PROVENANCE = FIXTURES / "provenance"

REDDIT_SOURCE = Path("/Users/peter/Downloads/export_ok_trade4308_20260723.zip")
X_SOURCE = Path(
    "/Users/peter/Downloads/twitter-2026-07-24-8d82ac9f4e2e22360c0fb2ae4ed5e5a0eb6caf93391aa6322092cbc2c80de690.zip"
)

REDACTION_TRANSFORM = (
    "Irreversible pseudonymization: stable SHA-256-truncated token maps for "
    "numeric/string IDs; synthetic handles/subreddits; benign placeholder text; "
    "example.invalid URLs; stripped IPs/emails/phones/media/DMs/ads/profile PII."
)

COMMENTS_HEADER = (
    "id,permalink,date,ip,subreddit,gildings,gildings_silver,gildings_supergold,"
    "link,parent,body,media"
)
POSTS_HEADER = "id,permalink,date,ip,subreddit,gildings,title,url,body"


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def token(prefix: str, value: str) -> str:
    digest = hashlib.sha256(f"ghostpost-fixture::{prefix}::{value}".encode()).hexdigest()
    if prefix in {"reddit_id", "reddit_post"}:
        return digest[:7]
    if prefix == "subreddit":
        return f"r_{digest[:8]}"
    if prefix == "x_id":
        return str(int(digest[:15], 16))
    if prefix == "handle":
        return f"user_{digest[:8]}"
    return digest[:12]


def redact_url(url: str) -> str:
    if not url:
        return url
    return f"https://example.invalid/{token('url', url)}"


def write_zip(path: Path, entries: dict[str, bytes | str]) -> bytes:
    path.parent.mkdir(parents=True, exist_ok=True)
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for name, payload in entries.items():
            if isinstance(payload, str):
                payload = payload.encode("utf-8")
            zf.writestr(name, payload)
    data = buf.getvalue()
    path.write_bytes(data)
    return data


def load_zip(path: Path) -> dict[str, bytes]:
    with zipfile.ZipFile(path) as zf:
        return {info.filename: zf.read(info.filename) for info in zf.infolist()}


def reddit_comments_csv(raw: bytes) -> str:
    text = raw.decode("utf-8")
    if text.startswith("\ufeff"):
        text = text[1:]
    reader = csv.DictReader(io.StringIO(text))
    assert reader.fieldnames
    id_map: dict[str, str] = {}
    rows_out: list[dict[str, str]] = []
    for index, row in enumerate(reader, start=1):
        old_id = row["id"]
        new_id = token("reddit_id", old_id)
        id_map[old_id] = new_id
        parent = row.get("parent") or ""
        new_parent = id_map.get(parent, token("reddit_id", parent) if parent else "")
        if parent and parent not in id_map:
            id_map[parent] = new_parent
        body = row.get("body") or ""
        if body in {"[deleted]", "[removed]", "[deleted by user]"}:
            new_body = body
        else:
            new_body = f"Synthetic comment body {index}."
        rows_out.append(
            {
                "id": new_id,
                "permalink": redact_url(row.get("permalink", "")),
                "date": row.get("date", ""),
                "ip": "",
                "subreddit": token("subreddit", row.get("subreddit", "")),
                "gildings": row.get("gildings", "0"),
                "gildings_silver": row.get("gildings_silver", "0"),
                "gildings_supergold": row.get("gildings_supergold", "0"),
                "link": redact_url(row.get("link", "")),
                "parent": new_parent,
                "body": new_body,
                "media": "",
            }
        )
    out = io.StringIO()
    writer = csv.DictWriter(out, fieldnames=reader.fieldnames, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows_out)
    return out.getvalue()


def reddit_posts_csv(raw: bytes) -> str:
    text = raw.decode("utf-8")
    if text.startswith("\ufeff"):
        text = text[1:]
    reader = csv.DictReader(io.StringIO(text))
    assert reader.fieldnames
    rows_out: list[dict[str, str]] = []
    for index, row in enumerate(reader, start=1):
        body = row.get("body") or ""
        title = row.get("title") or ""
        rows_out.append(
            {
                "id": token("reddit_post", row["id"]),
                "permalink": redact_url(row.get("permalink", "")),
                "date": row.get("date", ""),
                "ip": "",
                "subreddit": token("subreddit", row.get("subreddit", "")),
                "gildings": row.get("gildings", "0"),
                "title": title
                if title in {"[deleted]", "[removed]", "[deleted by user]"}
                else f"Synthetic post title {index}",
                "url": redact_url(row.get("url", "")),
                "body": body
                if body in {"[deleted]", "[removed]", "[deleted by user]"}
                else f"Synthetic post body {index}.",
            }
        )
    out = io.StringIO()
    writer = csv.DictWriter(out, fieldnames=reader.fieldnames, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows_out)
    return out.getvalue()


def reddit_statistics_csv(raw: bytes) -> str:
    rows: list[tuple[str, str]] = []
    reader = csv.reader(io.StringIO(raw.decode("utf-8")))
    for statistic, value in reader:
        if statistic == "statistic":
            rows.append((statistic, value))
        elif statistic in {"account name", "email address"}:
            rows.append((statistic, "redacted_account"))
        else:
            rows.append((statistic, value))
    out = io.StringIO()
    writer = csv.writer(out, lineterminator="\n")
    writer.writerows(rows)
    return out.getvalue()


def unwrap_js_array(raw: bytes) -> list:
    text = raw.decode("utf-8")
    eq = text.index("=")
    start = text.index("[", eq)
    end = text.rindex("]")
    return json.loads(text[start : end + 1])


def wrap_js(wrapper_name: str, payload: list) -> str:
    return f"window.YTD.{wrapper_name} = {json.dumps(payload, ensure_ascii=False, separators=(',', ': '))}"


def redact_tweet_object(tweet: dict, id_map: dict[str, str], index: int) -> dict:
    old_id = str(tweet.get("id_str") or tweet.get("id"))
    new_id = id_map.setdefault(old_id, token("x_id", old_id))
    tweet = json.loads(json.dumps(tweet))
    tweet["id_str"] = new_id
    tweet["id"] = int(new_id)
    text = tweet.get("full_text") or tweet.get("text") or ""
    if text.startswith("@"):
        tweet["full_text"] = f"@{token('handle', 'reply')} synthetic reply {index}"
    elif text.startswith("RT @"):
        tweet["full_text"] = f"RT @{token('handle', 'rt')}: synthetic retweet {index}"
    else:
        tweet["full_text"] = f"Synthetic tweet body {index}"
    if "text" in tweet:
        tweet["text"] = f"Synthetic classic tweet {index}"
    for key in (
        "in_reply_to_status_id_str",
        "in_reply_to_status_id",
        "quoted_status_id_str",
        "quoted_status_id",
        "in_reply_to_user_id_str",
        "in_reply_to_user_id",
    ):
        if key in tweet and tweet[key]:
            ref = str(tweet[key])
            mapped = id_map.get(ref, token("x_id", ref))
            tweet[key] = int(mapped) if key.endswith("_id") and not key.endswith("_str") else mapped
    if tweet.get("in_reply_to_screen_name"):
        tweet["in_reply_to_screen_name"] = token("handle", tweet["in_reply_to_screen_name"])
    entities = tweet.get("entities")
    if isinstance(entities, dict):
        for mention in entities.get("user_mentions", []):
            if "screen_name" in mention:
                mention["screen_name"] = token("handle", mention["screen_name"])
            if "name" in mention:
                mention["name"] = "Synthetic User"
            if "id_str" in mention:
                mention["id_str"] = token("x_id", mention["id_str"])
            if "id" in mention:
                mention["id"] = int(mention["id_str"])
        for url in entities.get("urls", []):
            if "url" in url:
                url["url"] = redact_url(url["url"])
            if "expanded_url" in url:
                url["expanded_url"] = redact_url(url["expanded_url"])
    for container_key in ("edit_info", "edit_control"):
        container = tweet.get(container_key)
        if not isinstance(container, dict):
            continue
        ids = container.get("editTweetIds") or container.get("edit_tweet_ids")
        if isinstance(ids, list):
            mapped = [id_map.get(str(item), token("x_id", str(item))) for item in ids]
            if "editTweetIds" in container.get("initial", {}):
                container["initial"]["editTweetIds"] = mapped
            else:
                container["edit_tweet_ids"] = mapped
    return tweet


def redact_x_rows(rows: list, id_map: dict[str, str]) -> list:
    out = []
    for index, row in enumerate(rows, start=1):
        if "tweet" in row:
            out.append({"tweet": redact_tweet_object(row["tweet"], id_map, index)})
        else:
            old_id = str(row["id"])
            new_id = id_map.setdefault(old_id, token("x_id", old_id))
            out.append(
                {
                    "id": int(new_id),
                    "text": f"Synthetic classic tweet {index}",
                    "created_at": row.get("created_at", "Thu Jul 23 12:00:00 +0000 2026"),
                }
            )
    return out


def redact_note_rows(rows: list, id_map: dict[str, str]) -> list:
    out = []
    for index, row in enumerate(rows, start=1):
        note = json.loads(json.dumps(row.get("noteTweet", row)))
        old_id = str(note.get("noteTweetId", index))
        note["noteTweetId"] = id_map.setdefault(old_id, token("x_id", old_id))
        core = note.get("core", {})
        if isinstance(core, dict) and "text" in core:
            core["text"] = f"Synthetic long-form note body {index}."
        out.append({"noteTweet": note})
    return out


def redact_manifest(raw: bytes) -> bytes:
    text = raw.decode("utf-8")
    text = re.sub(r'"userName"\s*:\s*"[^"]*"', '"userName" : "redacted_user"', text)
    text = re.sub(r'"displayName"\s*:\s*"[^"]*"', '"displayName" : "Redacted User"', text)
    text = re.sub(r'"accountId"\s*:\s*"[^"]*"', '"accountId" : "0000000000000000001"', text)
    return text.encode("utf-8")


def write_manifest(path: Path, **payload) -> None:
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def main() -> None:
    reddit = load_zip(REDDIT_SOURCE)
    comments_csv = reddit_comments_csv(reddit["comments.csv"])
    posts_csv = reddit_posts_csv(reddit["posts.csv"])
    statistics_csv = reddit_statistics_csv(reddit["statistics.csv"])

    comments_zip = write_zip(
        FIXTURES / "reddit" / "user_redacted_comments_v1.zip",
        {"comments.csv": comments_csv, "statistics.csv": statistics_csv},
    )
    posts_zip = write_zip(
        FIXTURES / "reddit" / "user_redacted_posts_v1.zip",
        {"posts.csv": posts_csv, "statistics.csv": statistics_csv},
    )

    x = load_zip(X_SOURCE)
    id_map: dict[str, str] = {}
    tweets = unwrap_js_array(x["data/tweets.js"])
    selected = tweets[:40]
    for row in tweets:
        tweet = row.get("tweet", row)
        if tweet.get("edit_info") or tweet.get("edit_control"):
            if row not in selected:
                selected.append(row)
        if len(selected) >= 60:
            break
    redacted_tweets = redact_x_rows(selected, id_map)
    deleted = unwrap_js_array(x["data/deleted-tweets.js"])
    notes = unwrap_js_array(x["data/note-tweet.js"])

    x_gdpr_zip = write_zip(
        FIXTURES / "x" / "user_redacted_gdpr_v1.zip",
        {
            "data/manifest.js": redact_manifest(x["data/manifest.js"]),
            "data/tweets.js": wrap_js("tweets.part0", redacted_tweets),
            "data/deleted-tweets.js": wrap_js("deleted_tweets.part0", redact_x_rows(deleted, id_map)),
            "data/note-tweet.js": wrap_js("note_tweet.part0", redact_note_rows(notes[:20], id_map)),
        },
    )

    classic_rows = []
    for index, row in enumerate(redacted_tweets[:12], start=1):
        tweet = row["tweet"]
        classic_rows.append(
            {
                "id": int(tweet["id_str"]),
                "text": f"Synthetic classic tweet {index}",
                "created_at": tweet.get("created_at", "Thu Jul 23 12:00:00 +0000 2026"),
            }
        )
    x_classic_zip = write_zip(
        FIXTURES / "x" / "user_redacted_classic_v1.zip",
        {"data/tweets/2026_07.js": wrap_js("tweets.part0", classic_rows)},
    )

    PROVENANCE.mkdir(parents=True, exist_ok=True)
    write_manifest(
        PROVENANCE / "RedditGdprCsv.manifest.json",
        family="RedditGdprCsv",
        fixture="reddit/user_redacted_comments_v1.zip",
        fixtureSha256=sha256_hex(comments_zip),
        exportGenerationDate="2026-07-23",
        redactionTransform=REDACTION_TRANSFORM,
        approvedForProduction=True,
        observedHeaders=[COMMENTS_HEADER],
    )
    write_manifest(
        PROVENANCE / "reddit_posts_provisional.manifest.json",
        family="RedditGdprCsv",
        fixture="reddit/user_redacted_posts_v1.zip",
        fixtureSha256=sha256_hex(posts_zip),
        exportGenerationDate="2026-07-23",
        redactionTransform=REDACTION_TRANSFORM,
        approvedForProduction=False,
        observedHeaders=[POSTS_HEADER],
    )
    write_manifest(
        PROVENANCE / "XGdpr.manifest.json",
        family="XGdpr",
        fixture="x/user_redacted_gdpr_v1.zip",
        fixtureSha256=sha256_hex(x_gdpr_zip),
        exportGenerationDate="2026-07-24",
        redactionTransform=REDACTION_TRANSFORM,
        approvedForProduction=True,
        observedWrappers=[
            "window.YTD.tweets.part0",
            "window.YTD.deleted_tweets.part0",
            "window.YTD.note_tweet.part0",
        ],
    )
    write_manifest(
        PROVENANCE / "XClassic.manifest.json",
        family="XClassic",
        fixture="x/user_redacted_classic_v1.zip",
        fixtureSha256=sha256_hex(x_classic_zip),
        exportGenerationDate="2026-07-24",
        redactionTransform=REDACTION_TRANSFORM,
        approvedForProduction=True,
        observedWrappers=["window.YTD.tweets.part0"],
    )
    print("done")


if __name__ == "__main__":
    main()
