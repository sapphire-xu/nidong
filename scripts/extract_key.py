import os, re, pathlib
p = pathlib.Path(os.environ["TEMP"]) / "login_neu.js"
t = p.read_text(encoding="utf-8", errors="replace")
m = re.search(r'publicKeyStr\s*=\s*"([A-Za-z0-9+/=]+)"', t)
print("found", bool(m), "len", len(m.group(1)) if m else 0)
if m:
    print(m.group(1))
    print("tail", m.group(1)[-20:])
m2 = re.search(r"MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA[A-Za-z0-9+/=]{200,}", t)
print("greedy len", len(m2.group(0)) if m2 else None)
if m2:
    print("greedy tail", repr(m2.group(0)[-50:]))
