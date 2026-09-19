#!/usr/bin/env python3
"""Seed an otelview instance with realistic demo telemetry over OTLP/HTTP.

Usage:
  scripts/demo-data.py                 # seed ~1h of history (traces+logs+metrics)
  scripts/demo-data.py --live          # then keep emitting one trace every ~2s
  scripts/demo-data.py --endpoint http://localhost:4318 --token sekret

Simulated system: gateway → checkout → {inventory, payments → psp, postgres}
plus a product-browsing path with a cache. ~10% of checkouts fail at the
payment step; logs are trace-correlated; metrics cover sum/gauge/histogram.
"""

import argparse
import json
import random
import time
import urllib.request

SERVICES = {
    "gateway": {"host": "gw-1"},
    "checkout": {"host": "app-1"},
    "payments": {"host": "app-2"},
    "inventory": {"host": "app-1"},
    "catalog": {"host": "app-3"},
    "redis": {"host": "cache-1"},
    "postgres": {"host": "db-1"},
    "notifications": {"host": "app-3"},
}

def rid(n):
    return "".join(random.choices("0123456789abcdef", k=n))

def kv(key, value):
    if isinstance(value, bool):
        v = {"boolValue": value}
    elif isinstance(value, int):
        v = {"intValue": str(value)}
    elif isinstance(value, float):
        v = {"doubleValue": value}
    else:
        v = {"stringValue": str(value)}
    return {"key": key, "value": v}

def resource(service):
    return {
        "attributes": [
            kv("service.name", service),
            kv("host.name", SERVICES[service]["host"]),
            kv("deployment.environment", "demo"),
        ]
    }

def span(trace, sid, parent, name, kind, t0, dur_ms, attrs=None, status=None, events=None):
    s = {
        "traceId": trace,
        "spanId": sid,
        "name": name,
        "kind": kind,
        "startTimeUnixNano": str(t0),
        "endTimeUnixNano": str(t0 + int(dur_ms * 1e6)),
        "attributes": [kv(k, v) for k, v in (attrs or {}).items()],
    }
    if parent:
        s["parentSpanId"] = parent
    if status:
        s["status"] = status
    if events:
        s["events"] = events
    return s

def post(endpoint, path, payload, token):
    req = urllib.request.Request(
        endpoint + path,
        data=json.dumps(payload).encode(),
        headers={"content-type": "application/json"},
        method="POST",
    )
    if token:
        req.add_header("x-otelview-token", token)
    with urllib.request.urlopen(req) as resp:
        resp.read()

def checkout_trace(t0):
    """One POST /checkout trace; returns (spans_by_service, logs, is_error)."""
    trace = rid(32)
    fail = random.random() < 0.10
    slow_db = random.random() < 0.15
    total = random.uniform(80, 250) + (300 if slow_db else 0)
    order = f"o-{random.randint(1000, 9999)}"
    user = f"u-{random.randint(1, 400)}"

    g = rid(16); c = rid(16); i = rid(16); p = rid(16); psp = rid(16)
    db1 = rid(16); db2 = rid(16); n = rid(16)
    ms = 1e6
    spans = {
        "gateway": [span(trace, g, None, "POST /checkout", 2, t0, total,
            {"http.method": "POST", "http.route": "/checkout",
             "http.status_code": 402 if fail else 201, "user.id": user},
            {"code": 2, "message": "payment declined"} if fail else {"code": 1})],
        "checkout": [span(trace, c, g, "create order", 1, t0 + int(8 * ms) // int(1e6) * int(1e6), total - 20,
            {"order.id": order, "order.items": random.randint(1, 6)},
            {"code": 2, "message": "charge failed"} if fail else None)],
        "inventory": [span(trace, i, c, "reserve stock", 1, t0 + int(15 * ms), random.uniform(8, 30),
            {"order.id": order})],
        "payments": [span(trace, p, c, "charge card", 3, t0 + int(50 * ms), total - 90,
            {"payment.provider": "stripe", "payment.amount": round(random.uniform(9, 300), 2)},
            {"code": 2, "message": "card declined"} if fail else {"code": 1},
            [{"name": "retry", "timeUnixNano": str(t0 + int(70 * ms)),
              "attributes": [kv("attempt", 2)]}] if random.random() < 0.3 else None)],
        "postgres": [
            span(trace, db1, c, "INSERT orders", 3, t0 + int(20 * ms),
                 random.uniform(3, 12) + (280 if slow_db else 0),
                 {"db.system": "postgresql", "db.statement": "INSERT INTO orders VALUES ($1, $2)"},
                 None,
                 [{"name": "lock.wait", "timeUnixNano": str(t0 + int(25 * ms)),
                   "attributes": [kv("relation", "orders")]}] if slow_db else None),
            span(trace, db2, p, "SELECT customers", 3, t0 + int(55 * ms), random.uniform(2, 9),
                 {"db.system": "postgresql", "db.statement": "SELECT * FROM customers WHERE id=$1"}),
        ],
        "notifications": [] if fail else [span(trace, n, c, "enqueue email", 4,
            t0 + int((total - 30) * ms), random.uniform(2, 10), {"queue": "emails"})],
    }
    spans["gateway"][0]["spanId"] = g
    # psp roundtrip under payments
    spans["payments"].append(span(trace, psp, p, "psp roundtrip", 3, t0 + int(60 * ms),
        (total - 90) * 0.7, {"net.peer.name": "api.stripe.com"}))

    logs = [("checkout", 9, "INFO", f"order {order} created for {user}", trace, c)]
    if fail:
        logs.append(("payments", 17, "ERROR", f"payment declined for {order}: card_declined", trace, p))
        logs.append(("gateway", 13, "WARN", f"POST /checkout responded 402 ({order})", trace, g))
    if slow_db:
        logs.append(("postgres", 13, "WARN", f"slow query 280ms: INSERT INTO orders ({order})", trace, db1))
    return spans, logs, fail

def browse_trace(t0):
    trace = rid(32)
    hit = random.random() < 0.7
    total = random.uniform(5, 20) if hit else random.uniform(25, 90)
    g = rid(16); cat = rid(16); r = rid(16); db = rid(16)
    ms = 1e6
    spans = {
        "gateway": [span(trace, g, None, "GET /products", 2, t0, total,
            {"http.method": "GET", "http.route": "/products", "http.status_code": 200})],
        "catalog": [span(trace, cat, g, "list products", 1, t0 + int(2 * ms), total - 4)],
        "redis": [span(trace, r, cat, "GET catalog:page", 3, t0 + int(4 * ms), random.uniform(0.5, 2),
            {"db.system": "redis", "cache.hit": hit})],
        "postgres": [] if hit else [span(trace, db, cat, "SELECT products", 3, t0 + int(8 * ms),
            total - 15, {"db.system": "postgresql", "db.statement": "SELECT * FROM products LIMIT 50"})],
    }
    logs = []
    if not hit:
        logs.append(("catalog", 5, "DEBUG", "cache miss for catalog:page, falling back to db", trace, cat))
    return spans, logs, False

def send_traces(endpoint, token, spans_by_service):
    rs = [{"resource": resource(svc), "scopeSpans": [{"scope": {"name": "demo"}, "spans": spans}]}
          for svc, spans in spans_by_service.items() if spans]
    post(endpoint, "/v1/traces", {"resourceSpans": rs}, token)

def send_logs(endpoint, token, logs):
    by_svc = {}
    for svc, num, text, body, trace, span_id in logs:
        by_svc.setdefault(svc, []).append({
            "timeUnixNano": str(now_ns()),
            "severityNumber": num,
            "severityText": text,
            "body": {"stringValue": body},
            "traceId": trace or "",
            "spanId": span_id or "",
            "attributes": [kv("env", "demo")],
        })
    rl = [{"resource": resource(svc), "scopeLogs": [{"scope": {"name": "demo"}, "logRecords": recs}]}
          for svc, recs in by_svc.items()]
    post(endpoint, "/v1/logs", {"resourceLogs": rl}, token)

def now_ns():
    return int(time.time() * 1e9)

def send_metrics(endpoint, token, minutes):
    """One batch: per-minute points for the past `minutes` minutes."""
    t_now = now_ns()
    rms = []
    counters = {}
    for svc in ["gateway", "checkout", "payments", "catalog"]:
        metrics = []
        req_points, dur_points, cpu_points, q_points = [], [], [], []
        for m in range(minutes, -1, -1):
            t = str(t_now - m * 60_000_000_000)
            for route in ["/checkout", "/products", "/healthz"]:
                key = (svc, route)
                counters[key] = counters.get(key, random.randint(100, 400)) + random.randint(5, 60)
                req_points.append({"timeUnixNano": t, "asInt": str(counters[key]),
                                   "attributes": [kv("http.route", route)]})
                base = {"/checkout": 120, "/products": 30, "/healthz": 2}[route]
                buckets = [random.randint(0, 20) for _ in range(5)]
                dur_points.append({
                    "timeUnixNano": t,
                    "count": str(sum(buckets)),
                    "sum": sum(buckets) * base * random.uniform(0.7, 1.4),
                    "bucketCounts": [str(b) for b in buckets],
                    "explicitBounds": [10, 50, 100, 500],
                    "attributes": [kv("http.route", route)],
                })
            cpu_points.append({"timeUnixNano": t,
                               "asDouble": min(0.95, max(0.03, random.gauss(0.35, 0.15)))})
            q_points.append({"timeUnixNano": t, "asInt": str(max(0, int(random.gauss(12, 8))))})
        metrics.append({"name": "http.server.requests", "unit": "1",
                        "description": "completed requests",
                        "sum": {"aggregationTemporality": 2, "isMonotonic": True,
                                "dataPoints": req_points}})
        metrics.append({"name": "http.server.duration", "unit": "ms",
                        "description": "request latency",
                        "histogram": {"aggregationTemporality": 2, "dataPoints": dur_points}})
        metrics.append({"name": "process.cpu.utilization", "unit": "1",
                        "description": "cpu fraction", "gauge": {"dataPoints": cpu_points}})
        metrics.append({"name": "queue.depth", "unit": "{jobs}",
                        "description": "pending jobs", "gauge": {"dataPoints": q_points}})
        rms.append({"resource": resource(svc),
                    "scopeMetrics": [{"scope": {"name": "demo"}, "metrics": metrics}]})
    post(endpoint, "/v1/metrics", {"resourceMetrics": rms}, token)

def seed_history(endpoint, token, minutes=60, traces=80):
    t_now = now_ns()
    for k in range(traces):
        t0 = t_now - random.randint(0, minutes * 60) * 1_000_000_000
        spans, logs, _ = checkout_trace(t0) if random.random() < 0.5 else browse_trace(t0)
        send_traces(endpoint, token, spans)
        if logs:
            send_logs(endpoint, token, logs)
    # background service chatter
    chatter = []
    for _ in range(60):
        svc = random.choice(list(SERVICES))
        lvl = random.choices([(5, "DEBUG"), (9, "INFO"), (13, "WARN"), (17, "ERROR"), (21, "FATAL")],
                             weights=[30, 50, 12, 6, 1])[0]
        chatter.append((svc, lvl[0], lvl[1],
                        random.choice([
                            "connection pool at %d%% capacity" % random.randint(20, 95),
                            "gc pause %dms" % random.randint(2, 40),
                            "config reloaded",
                            "healthcheck ok",
                            "tls handshake completed",
                            "worker heartbeat",
                        ]), None, None))
    send_logs(endpoint, token, chatter)
    send_metrics(endpoint, token, minutes)
    print(f"seeded {traces} traces, logs and {minutes}m of metrics")

def live(endpoint, token):
    print("live mode: one trace every ~2s (ctrl-c to stop)")
    n = 0
    while True:
        spans, logs, err = checkout_trace(now_ns()) if random.random() < 0.5 else browse_trace(now_ns())
        send_traces(endpoint, token, spans)
        if logs:
            send_logs(endpoint, token, logs)
        n += 1
        if n % 15 == 0:
            send_metrics(endpoint, token, 0)
        time.sleep(random.uniform(1.2, 3.0))

if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--endpoint", default="http://127.0.0.1:4318")
    ap.add_argument("--token", default="")
    ap.add_argument("--live", action="store_true")
    ap.add_argument("--minutes", type=int, default=60)
    ap.add_argument("--traces", type=int, default=80)
    args = ap.parse_args()
    seed_history(args.endpoint, args.token, args.minutes, args.traces)
    if args.live:
        live(args.endpoint, args.token)
