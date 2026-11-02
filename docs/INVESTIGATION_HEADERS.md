# Duplicate header rejection — 2026-11-02

The supplied session `httpied-debug-1793573793914-98028` has 37 lab responses with HTTP 400
and one earlier lab response with HTTP 200. The temporary project's saved captures were
read through a read-only SQLite connection; no project contents were modified.

The rejected responses contain the exact body:

```json
{"error":"Duplicate header names are not allowed"}
```

All 37 rejected outgoing requests contain two Cookie field lines. Their other header names
are unique, including one Host. The successful request has no duplicate header names.
The project's rewrite-rule list is empty. Example captures are IDs 18, 19 and 20. Cookie
values are deliberately omitted here. Summary records identify the corresponding normalized
HTTP/2 streams, successful upstream HTTP transport and the received 400 responses.

HTTP/2 permits Cookie values to be split into multiple fields for compression. Before
forwarding to HTTP/1.1, [RFC 9113 section 8.2.3](https://datatracker.ietf.org/doc/html/rfc9113#section-8.2.3)
requires concatenating them using semicolon-space. HTTPied's adapter failed to do this.
It now writes one Cookie field with those opaque values joined in their original order and
records the conversion in capture transformations. Cookie pairs are not parsed or deduplicated.
Raw HTTP/1 requests retain their original Cookie lines; other duplicate fields and response
Set-Cookie values retain their existing behavior.

The strict local HTTP/1 fixture reproduced status 400 before the fix and returns 200 after
the fix for split Cookie fields. It covers absent/single/multiple fields, repeated cookie
names, existing semicolons, escaped-value text and quoted commas; checks exact forwarded
values/captures, one Host, preserved repeated custom fields and two response Set-Cookie fields.
The verified local TLS/HTTP2 fixture checks joined cookies across all nine multiplexed streams.
The raw HTTP/1 forwarding regression checks that intentionally duplicate Cookie fields remain
byte-preserved. No requests were sent to the public lab while fixing this issue.
All 87 debug/release tests pass, including these cases and TLS verification regressions.

This session has no DNS deadlines. It also records four downstream TLS handshake EOFs
for browser background-service authorities before the lab traffic. Those identify the
browser-to-proxy handshake stage but do not explain the lab server's explicit HTTP 400
responses; their underlying cause remains separate and unconfirmed.

Next validation: reload the lab in the rebuilt app, inspect Outgoing for one joined Cookie
field and check the new summary if any header rejection remains. Existing saved captures
retain their historical contents.
