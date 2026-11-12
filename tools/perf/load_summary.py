"""Markdown table of load_bench runs.

    python tools/perf/load_summary.py tools/perf/results/<file>.json [...]

Times are milliseconds from the RakNet connection; for several players the
slowest player is shown. Flight columns: mean / worst share of the render disk
missing while flying, and the time to complete it after stopping.
"""

import json
import sys


def fmt(value, digits=0):
    if value is None:
        return "-"
    return f"{value:.{digits}f}"


def row(run):
    players = run["players_result"]
    join = max(p["join_ms"] for p in players)
    first = max((p["first_chunk_ms"] or 0) for p in players)
    disk = run["worst_disk_view_ms"]
    square = run["worst_square_view_ms"]
    # Columns per second until the view the server streams was complete.
    chunks = players[0].get("columns_received") or players[0]["square_chunks"]
    done = square or disk
    rate = chunks / (done / 1e3) if done else None
    flights = [p["flight"] for p in players if p.get("flight")]
    if flights:
        mean = sum(f["missing_disk_mean"] for f in flights) / len(flights)
        worst = max(f["missing_disk_max"] for f in flights)
        catches = [f["catch_up_ms"] for f in flights]
        catch = None if None in catches else max(catches)
        flight = f"{mean:.1%} / {worst:.1%} / {fmt(catch)}"
    else:
        flight = ""
    return (
        f"| {run['label']} | {run['generator']} | {run['world']} | {run['radius']} | "
        f"{run['players']} | {fmt(join)} | {fmt(first)} | {fmt(disk)} | {fmt(square)} | "
        f"{fmt(rate)} | {flight} |"
    )


def main():
    print(
        "| build | generator | world | radius | players | join | first chunk | "
        "disk view | square view | chunks/s | flight missing mean / max / catch-up |"
    )
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    for path in sys.argv[1:]:
        with open(path, encoding="utf-8") as f:
            for run in json.load(f):
                print(row(run))


if __name__ == "__main__":
    main()
