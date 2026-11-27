#!/usr/bin/env python3
"""Development-only inert resource; never used for production model delivery."""
import re
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DATA = bytes(range(256)) * 32768
ETAG = '"ethiopic-fixture-v1"'


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/fixture.bin":
            self.send_error(404)
            return
        start = 0
        requested = self.headers.get("Range")
        if requested and self.headers.get("If-Range", ETAG) == ETAG:
            match = re.fullmatch(r"bytes=(\d+)-", requested)
            if not match or int(match[1]) >= len(DATA):
                self.send_error(416)
                return
            start = int(match[1])
        self.send_response(206 if start else 200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(DATA) - start))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("ETag", ETAG)
        if start:
            self.send_header("Content-Range", f"bytes {start}-{len(DATA)-1}/{len(DATA)}")
        self.end_headers()
        try:
            for offset in range(start, len(DATA), 65536):
                self.wfile.write(DATA[offset:offset + 65536])
                self.wfile.flush()
                time.sleep(0.08)
        except (BrokenPipeError, ConnectionResetError):
            pass


if __name__ == "__main__":
    print("Development fixture: http://127.0.0.1:18765/fixture.bin (8 MiB)", flush=True)
    ThreadingHTTPServer(("127.0.0.1", 18765), Handler).serve_forever()
