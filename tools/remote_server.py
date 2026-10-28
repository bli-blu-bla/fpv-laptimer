"""A WebSocket server for the laptimer's server connection (API & Hooks → Server):
checks the login and prints every message the timer sends, as it comes.

    python3 -m venv tools/.venv-remote && tools/.venv-remote/bin/pip install websockets
    tools/.venv-remote/bin/python tools/remote_server.py --token geheim
    tools/.venv-remote/bin/python tools/remote_server.py --token geheim --cert fullchain.pem --key privkey.pem

Without --cert/--key it serves plain ws://, with them wss://. The timer only
trusts certificates of public CAs (webpki roots, e.g. Let's Encrypt), not
self-signed ones; for a quick test in the LAN use ws://.

Login with --token: either the Authorization header ("Bearer <token>" or the
token alone) or the first message {"type": "auth", "token": "<token>"};
otherwise the connection is closed with 4001. Without --token everyone may
connect. Protocol: docs/API.md, "Server-Verbindung".

GPL-3.0-or-later.
"""
import argparse
import asyncio
import json
import ssl
from datetime import datetime

import websockets

# How long the timer has for its auth message when the header didn't match, s.
AUTH_TIMEOUT_S = 5


def log(peer, text):
    print(f"{datetime.now():%H:%M:%S.%f}"[:-3], peer, text, flush=True)


async def check_auth(ws, token, peer):
    """True if the timer logged in: by header, or by its first message."""
    header = ws.request.headers.get("Authorization", "")
    if header in (f"Bearer {token}", token):
        log(peer, "angemeldet (Authorization-Header)")
        return True
    try:
        first = await asyncio.wait_for(ws.recv(), AUTH_TIMEOUT_S)
    except asyncio.TimeoutError:
        log(peer, "abgelehnt: keine Anmeldung")
        return False
    log(peer, first)
    try:
        msg = json.loads(first)
    except ValueError:
        msg = {}
    if msg.get("type") == "auth" and msg.get("token") == token:
        log(peer, "angemeldet (Token)")
        await ws.send(json.dumps({"type": "auth_ok"}))
        return True
    log(peer, "abgelehnt: falscher Token")
    return False


def handler(token):
    async def timer(ws):
        peer = "%s:%s" % ws.remote_address[:2]
        agent = ws.request.headers.get("User-Agent", "?")
        log(peer, f"verbunden ({agent})")
        if token and not await check_auth(ws, token, peer):
            await ws.close(4001, "auth")
            return
        try:
            async for msg in ws:
                log(peer, msg)
        except websockets.ConnectionClosed:
            pass
        log(peer, f"getrennt ({' '.join(str(x) for x in (ws.close_code, ws.close_reason) if x)})")

    return timer


async def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--host", default="0.0.0.0")
    p.add_argument("--port", type=int, default=8770)
    p.add_argument("--token", help="Token oder Header-Wert, den der Timer schicken muss")
    p.add_argument("--cert", help="Zertifikat (PEM, mit Kette) für wss://")
    p.add_argument("--key", help="Privater Schlüssel (PEM) für wss://")
    args = p.parse_args()

    tls = None
    if args.cert or args.key:
        if not (args.cert and args.key):
            p.error("--cert und --key nur zusammen")
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(args.cert, args.key)

    scheme = "wss" if tls else "ws"
    async with websockets.serve(handler(args.token), args.host, args.port, ssl=tls, max_size=None):
        print(f"Warte auf den Timer: {scheme}://{args.host}:{args.port}/", flush=True)
        await asyncio.Future()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
