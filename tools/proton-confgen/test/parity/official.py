"""Records what the official ProtonVPN Linux client puts on the wire.

Drives Proton's real packages (python-proton-core, python-proton-vpn-api-core)
against a local recording server and writes each raw request to testdata/, plus
the TLS ClientHello. Those files are the goldens the Go tests compare against.
Run through `make parity-goldens`; it needs the image from the Dockerfile.

Only the base URL is overridden. Headers, bodies, TLS setup and call order all
come from the client's own code.
"""
import asyncio
import http.client
import os
import pathlib
import re
import socket
import threading

from proton.session.environments import ProdEnvironment
from proton.vpn.core.session_holder import ClientTypeMetadata, SessionHolder
from proton.vpn.session import VPNSession

HERE = pathlib.Path(__file__).parent
OUT = HERE / "testdata"
AUTH_INFO = (OUT / "auth_info_response.json").read_bytes()
PORT, TLS_PORT, LIVE_PORT = 18201, 18202, 18203
USERNAME, PASSWORD = "parityuser", "parity-password"


def respond(path: str) -> bytes:
    if path == "/auth/info":
        return AUTH_INFO
    if path == "/auth":
        # A wrong-password reply: the request under test has already been sent.
        return b'{"Code": 8002, "Error": "Incorrect login credentials. Please try again"}'
    if path == "/auth/2fa":
        return b'{"Code": 1000, "Scopes": ["vpn"]}'
    if path == "/auth/refresh":
        return b'{"Code": 1000, "AccessToken": "TOKEN2", "RefreshToken": "REFRESH2", "Scopes": ["vpn"]}'
    return b'{"Code": 1000}'


def serve(port: int, hello_only: bool, records: list, live: bool = False):
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", port))
    srv.listen(8)

    def loop():
        while True:
            conn, _ = srv.accept()
            conn.settimeout(2.0)
            data = b""
            try:
                while True:
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                    if hello_only:
                        break
                    head, sep, body = data.partition(b"\r\n\r\n")
                    if sep:
                        length = 0
                        for line in head.split(b"\r\n"):
                            if line.lower().startswith(b"content-length:"):
                                length = int(line.split(b":")[1])
                        if len(body) >= length:
                            break
            except OSError:
                pass
            records.append(data)
            if live:
                conn.sendall(forward(data))
            elif not hello_only:
                path = data.split(b" ")[1].decode()
                payload = respond(path)
                status = b"422 Unprocessable Entity" if path == "/auth" else b"200 OK"
                conn.sendall(b"HTTP/1.1 " + status + b"\r\nContent-Type: application/json\r\nContent-Length: "
                             + str(len(payload)).encode() + b"\r\n\r\n" + payload)
            conn.close()

    threading.Thread(target=loop, daemon=True).start()


def forward(raw: bytes) -> bytes:
    """Relays a recorded request to the real API with real credentials swapped
    in, so the client under test receives genuine responses to parse."""
    head, _, body = raw.partition(b"\r\n\r\n")
    lines = head.decode().split("\r\n")
    method, path, _ = lines[0].split(" ")
    headers = {}
    for line in lines[1:]:
        name, _, value = line.partition(": ")
        if name.lower() != "host":
            headers[name] = value
    headers["x-pm-uid"] = os.environ["PARITY_UID"]
    headers["Authorization"] = "Bearer " + os.environ["PARITY_TOKEN"]
    conn = http.client.HTTPSConnection("vpn-api.proton.me", timeout=30)
    conn.request(method, path, body=body or None, headers=headers)
    resp = conn.getresponse()
    payload = resp.read()
    out = f"HTTP/1.1 {resp.status} {resp.reason}\r\nContent-Length: {len(payload)}\r\n"
    for name in ("Content-Type", "Content-Encoding", "Last-Modified"):
        if resp.getheader(name):
            out += f"{name}: {resp.getheader(name)}\r\n"
    return out.encode() + b"\r\n" + payload


def environment(url: str):
    class ParityEnvironment(ProdEnvironment):  # keeps production TLS pinning
        @property
        def http_base_url(self):
            return url
    return ParityEnvironment()


def new_session(url: str) -> VPNSession:
    # The exact header values the GUI client computes for itself.
    holder = SessionHolder(ClientTypeMetadata(type="gui"))
    sso = holder._proton_sso  # pylint: disable=protected-access
    session = VPNSession(
        appversion=sso._appversion,  # pylint: disable=protected-access
        user_agent=sso._user_agent,  # pylint: disable=protected-access
        timezone=holder._timezone,  # pylint: disable=protected-access
    )
    session.environment = environment(url)
    return session


def authenticate(session: VPNSession):
    for attr, value in (("UID", "UIDVALUE"), ("AccessToken", "TOKENVALUE"),
                        ("RefreshToken", "REFRESHVALUE"), ("Scopes", ["twofactor"]),
                        ("AccountName", USERNAME)):
        setattr(session, f"_Session__{attr}", value)


async def main():
    records, hellos = [], []
    serve(PORT, False, records)
    serve(TLS_PORT, True, hellos)
    base = f"http://127.0.0.1:{PORT}"

    # Fresh login: transport probe, SRP info, SRP proof.
    session = new_session(base)
    result = await session.login(USERNAME, PASSWORD)
    assert not result.success, "the mock rejects the password"

    # Authenticated calls on an established session.
    session = new_session(base)
    authenticate(session)
    await session.provide_2fa_code("123456")
    await session.async_refresh()

    # What the client fetches right after a successful login. These need real
    # responses to parse, so they are relayed to the live API when credentials
    # are provided; without them the existing goldens are left alone.
    synced = []
    if os.environ.get("PARITY_TOKEN"):
        serve(LIVE_PORT, False, synced, live=True)
        session = new_session(f"http://127.0.0.1:{LIVE_PORT}")
        authenticate(session)
        setattr(session, "_Session__Scopes", ["vpn"])
        await session.fetch_session_data()

    # The production TLS path, pinning included. The handshake cannot complete
    # against a plain socket, which is fine: only the ClientHello is wanted.
    session = new_session(f"https://localhost:{TLS_PORT}")
    try:
        await session.async_api_request("/tests/ping")
    except Exception:  # pylint: disable=broad-except
        pass
    await asyncio.sleep(0.3)

    # Every new session probes the transport first, hence the second ping.
    names = ["ping", "auth_info", "auth", None, "auth_2fa", "auth_refresh"]
    assert len(records) == len(names), f"expected {len(names)} requests, recorded {len(records)}"
    for name, raw in zip(names, records):
        print(raw.split(b"\r\n")[0].decode(), "->", name)
        if name:
            (OUT / f"{name}.http").write_bytes(raw)
    sync_names = {
        "/vpn/v2": "sync_vpn_info",
        "/vpn/v1/certificate": "sync_certificate",
        "/vpn/v1/location": "sync_location",
        "/vpn/v2/clientconfig": "sync_clientconfig",
        "/feature/v2/frontend": "sync_features",
        "/vpn/v1/logicals": "sync_logicals",
        "/core/v4/notifications": "sync_notifications",
    }
    order = []
    for raw in synced:
        path = raw.split(b" ")[1].decode().split("?")[0]
        if path == "/tests/ping":
            # A session used concurrently before its transport is chosen probes
            # once per call. A real login has already probed by this point.
            continue
        # The netzone is the caller's own network; keep it out of the repository.
        raw = re.sub(rb"(X-PM-netzone: )[^\r]+", rb"\g<1>192.0.2.0", raw)
        (OUT / f"{sync_names[path]}.http").write_bytes(raw)
        order.append(sync_names[path])
    if order:
        # The first four go out concurrently; only the order after them is fixed.
        print("sync order:", sorted(order[:4]), "then", order[4:])
        (OUT / "sync_order.txt").write_text("\n".join(sorted(order[:4]) + order[4:]) + "\n")
    # Written next to the Go code that embeds it.
    (HERE / "clienthello.bin").write_bytes(hellos[0])
    print("clienthello", len(hellos[0]), "bytes")


asyncio.run(main())
