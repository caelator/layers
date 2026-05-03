import json

print(json.dumps({
    "route": "targeted_preflight_smoke",
    "confidence": "smoke_only",
    "warning": "synthetic packet for runner execution smoke; not product-effectiveness evidence"
}, sort_keys=True))
