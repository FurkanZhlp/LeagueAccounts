"""Small, opt-in public-profile experiment; no account file or credentials are read.

py -3 tools/provider_probe.py --renew --parallel 2 --mcp --leagueofgraphs
Raw responses and timings go to the specified output directory, outside normal tests.
"""
import argparse
import concurrent.futures
import datetime
import html
import json
import pathlib
import re
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--profile", action="append", help="region:Game Name#TAG (public IDs only)")
parser.add_argument("--renew", action="store_true")
parser.add_argument("--parallel", type=int, choices=[1, 2, 3], default=2)
parser.add_argument("--mcp", action="store_true")
parser.add_argument("--leagueofgraphs", action="store_true")
parser.add_argument("--output", default="target/provider-probe")
args = parser.parse_args()
out = pathlib.Path(args.output)
out.mkdir(parents=True, exist_ok=True)
lock = threading.Lock()
events = []
headers = {"User-Agent": "Mozilla/5.0", "Accept-Language": "en-US,en;q=0.9"}


def request(label, url, payload=None, extra=None):
    started = time.monotonic()
    h = headers | (extra or {})
    try:
        response = urllib.request.urlopen(urllib.request.Request(url, data=payload, headers=h), timeout=15)
    except urllib.error.HTTPError as error:
        response = error
    except (OSError, TimeoutError) as error:
        record = {"label": label, "url": url, "error": type(error).__name__, "seconds": round(time.monotonic() - started, 3)}
        with lock:
            events.append(record)
            print(json.dumps(record), flush=True)
        return {}, ""
    body = response.read().decode(errors="replace")
    record = {"label": label, "url": url, "method": "POST" if payload is not None else "GET", "status": response.status,
              "seconds": round(time.monotonic() - started, 3), "type": response.headers.get("Content-Type"),
              "server_date": response.headers.get("Date"), "retry_after": response.headers.get("Retry-After"),
              "challenge": "Just a moment" in body or "cf-chl-" in body}
    (out / (re.sub(r"[^a-zA-Z0-9_-]", "_", label) + ".txt")).write_text(body, encoding="utf-8")
    with lock:
        events.append(record)
        print(json.dumps(record), flush=True)
    return dict(response.headers), body


def field(body, name):
    found = re.search('"' + name + r'"\s*:\s*"([^"]+)"', html.unescape(body).replace('\\"', '"'))
    return found.group(1) if found else None


def snapshot(body):
    result = {key: field(body, key) for key in ["initUpdatedAt", "initRenewableAt", "dateModified"]}
    # Other players' PUUIDs can occur earlier in the page. Use the Update component.
    payload = html.unescape(body).replace('\\"', '"')
    component = re.search(r'\{"puuid":"[^"]+","initRenewableAt":', payload)
    result["puuid"] = json.JSONDecoder().raw_decode(payload[component.start():])[0]["puuid"] if component else None
    return result


def actions(body):
    scripts = re.findall(r'<script[^>]+src="([^"]+\.js)"', body)
    result = {}
    # Last-loaded application chunks first; stop as soon as both references are found.
    for index, url in enumerate(reversed(scripts)):
        if not url.startswith("https://c-lol-web.op.gg/"):
            continue
        _, bundle = request("bundle-" + str(index), url)
        for identifier, name in re.findall(r'createServerReference\)\("([a-f0-9]{40,})",.{0,160}?"(renewalStatus|renewal)"\)', bundle):
            result[name] = identifier
        if len(result) == 2:
            return result
    raise RuntimeError("Current renewal action references not found; refusing to guess")


def action(label, url, identifier, arguments):
    _, body = request(label, url, json.dumps([arguments]).encode(),
                      {"Next-Action": identifier, "Origin": "https://op.gg", "Accept": "text/x-component", "Content-Type": "text/plain;charset=UTF-8"})
    for line in body.splitlines():
        try:
            value = json.loads(line.split(":", 1)[1])
            if isinstance(value, dict) and "status" in value:
                return value
        except (ValueError, IndexError):
            pass
    return {"status": "unrecognized_response"}


profiles = args.profile or ["kr:Hide on bush#KR1", "kr:Gumayusi#KR1", "kr:Oner#KR1"]
if len(profiles) > 3:
    parser.error("Use at most three public profiles per experiment")
urls = []
for profile in profiles:
    region, riot_id = profile.split(":", 1)
    name, tag = riot_id.rsplit("#", 1)
    urls.append((region, name, tag, "https://op.gg/lol/summoners/" + region + "/" + urllib.parse.quote(name, safe="") + "-" + urllib.parse.quote(tag, safe="")))
_, first_body = request("discovery-profile", urls[0][3])
references = actions(first_body) if args.renew else {}
print("action references:", references)


def probe(item):
    region, name, tag, url = item
    label = region + "-" + name
    _, before = request(label + "-before", url)
    result = {"profile": region + ":" + name + "#" + tag, "before": snapshot(before)}
    if args.renew and result["before"]["puuid"]:
        started = time.monotonic()
        arguments = {"region": region, "puuid": result["before"]["puuid"], "isPremiumPrimary": False}
        status = action(label + "-renewal", url, references["renewal"], arguments)
        poll = 0
        while status.get("status") == "RENEWING" and time.monotonic() - started < 20:
            time.sleep(min(max(status.get("delay", 1000) / 1000, 0.5), 3))
            poll += 1
            status = action(label + "-poll-" + str(poll), url, references["renewalStatus"], {"region": region, "puuid": arguments["puuid"]})
        result["renewal"] = status
        result["renewal_seconds"] = round(time.monotonic() - started, 3)
        _, after = request(label + "-after", url)
        result["after"] = snapshot(after)
        _, repeat = request(label + "-repeat", url)
        result["repeat"] = snapshot(repeat)
    if args.leagueofgraphs:
        request(label + "-leagueofgraphs", "https://www.leagueofgraphs.com/summoner/" + region + "/" + urllib.parse.quote_plus(name) + "-" + urllib.parse.quote(tag, safe=""))
    return result


with concurrent.futures.ThreadPoolExecutor(max_workers=args.parallel) as pool:
    results = list(pool.map(probe, urls))
if args.mcp:
    endpoint = "https://mcp-api.op.gg/mcp"
    h = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}

    def rpc(label, method, params, identifier):
        return request(label, endpoint, json.dumps({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params}).encode(), h)

    rh, _ = rpc("mcp-initialize", "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "leagueaccounts-probe", "version": "1"}}, 1)
    h["Mcp-Session-Id"] = rh.get("Mcp-Session-Id", "")
    h["MCP-Protocol-Version"] = "2025-06-18"
    _, tools = rpc("mcp-tools", "tools/list", {}, 2)
    print("MCP tools:", [tool["name"] for tool in json.loads(tools).get("result", {}).get("tools", [])])
    rpc("mcp-historical-renewal", "tools/call", {"name": "lol-summoner-renewal", "arguments": {}}, 3)
    region, name, tag, _ = urls[0]
    rpc("mcp-profile", "tools/call", {"name": "lol_get_summoner_profile", "arguments": {
        "region": region, "game_name": name, "tag_line": tag,
        "desired_output_fields": ["data.summoner.{game_name,tagline,level,updated_at,renewable_at}", "data.summoner.league_stats[].{game_type,win,lose}", "data.summoner.league_stats[].tier_info.{tier,division,lp}"]}}, 4)
(out / "summary.json").write_text(json.dumps({"at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "profiles": results, "actions": references, "requests": events}, indent=2), encoding="utf-8")
print(json.dumps(results, indent=2))
