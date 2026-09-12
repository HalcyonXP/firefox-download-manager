"""Fixed benign candidate cases on the existing retained loopback server."""
from .capture import BODY
from .fixture import Handler

FILES = frozenset({"direct", "cross", "off", "restart", "post", "frame", "blob", "newtab",
                   "cookie", "set-cookie", "vary", "revoked", "denied", "regranted", "container", "private"})


def filename(case):
    if case not in FILES:
        raise RuntimeError("unknown candidate fixture file")
    return f"candidate-{case}.bin"


class CandidateHandler(Handler):
    def do_POST(self):
        if self.path != "/file/post" or self.headers.get("Content-Length") != "0":
            self.send_error(400)
            return
        self.reply(True)

    def reply(self, body):
        path = self.path
        kind = path.removeprefix("/page/")
        status, data, mime, headers = 200, BODY, "application/octet-stream", []
        if path.startswith("/page/") and kind in FILES | {"navigation"}:
            target = f"/file/{kind}"
            link = f'<a id="download" href="{target}">Download fixture</a>'
            if kind == "cross":
                link = '<a id="download" href="/redirect">Redirect fixture</a>'
            elif kind == "cookie":
                headers.append(("Set-Cookie", "candidate_fixture=synthetic; Path=/file/cookie; HttpOnly; SameSite=Lax"))
            elif kind == "post":
                link = '<form method="post" action="/file/post"><button id="download">POST fixture</button></form>'
            elif kind == "frame":
                link = '<iframe id="frame" src="/frame-page"></iframe>'
            elif kind == "blob":
                link = '<a id="download" download="candidate-blob.bin">Blob fixture</a><script>document.querySelector("a").href=URL.createObjectURL(new Blob(["owned capture fixture\\n".repeat(64)],{type:"application/octet-stream"}));</script>'
            elif kind == "newtab":
                link = '<a id="download" target="_blank" href="/file/newtab">New tab fixture</a>'
            elif kind == "navigation":
                link = '<a id="download" href="/navigation">Normal page</a>'
            data = ('<!doctype html><meta charset="utf-8"><title>Owned candidate fixture</title>' + link).encode()
            mime = "text/html; charset=utf-8"
        elif path == "/frame-page":
            data = b'<!doctype html><title>Owned inner frame</title><a id="download" href="/file/frame">Frame download</a>'
            mime = "text/html; charset=utf-8"
        elif path == "/navigation":
            data, mime = b'<!doctype html><title>Owned ordinary navigation</title><p>Navigation intact</p>', "text/html; charset=utf-8"
        elif path == "/redirect":
            status, data = 302, b""
            headers.append(("Location", self.server.fixture.candidate_target))
        elif path.startswith("/file/") and path.removeprefix("/file/") in FILES:
            kind = path.removeprefix("/file/")
            headers.append(("Content-Disposition", f'attachment; filename="{filename(kind)}"'))
            if kind == "set-cookie": headers.append(("Set-Cookie", "candidate_response=synthetic; Path=/unused; HttpOnly"))
            if kind == "vary": headers.append(("Vary", "Cookie"))
            if kind == "cookie" and self.headers.get("Cookie") != "candidate_fixture=synthetic":
                status, data = 403, b""
        else:
            status, data = 404, b""
        # Never retain header values or arbitrary paths. Bound the fixed-case request count.
        route = path if path in {*("/page/"+k for k in FILES | {"navigation"}),
                                 *("/file/"+k for k in FILES), "/redirect", "/frame-page", "/navigation"} else "other"
        with self.server.fixture.lock:
            self.server.fixture.requests[(self.command, route)] += 1
            overflow = sum(self.server.fixture.requests.values()) > 256
        if overflow:
            status, data, headers = 429, b"", []
        self.send_response(status)
        self.send_header("Content-Type", mime)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Connection", "close")
        for name, value in headers: self.send_header(name, value)
        self.end_headers()
        if body: self.wfile.write(data)
