# Reverse proxy / TLS samples

`tock-server` speaks plain HTTP (`TOCK_BIND`); the `tock-web` container serves
the console and proxies the API to the server. Put one of these in front to
terminate HTTPS for a public deployment. All three route the same way — TLS in,
plain HTTP to `tock-web` — so pick whichever you already run.

| Sample                       | Best for                              | ACME (auto-TLS) |
| ---------------------------- | ------------------------------------- | --------------- |
| [`Caddyfile`](Caddyfile)     | Simplest; bundled in `docker-compose` | Yes, built in   |
| [`nginx.conf`](nginx.conf)   | Hosts already running nginx           | Via certbot     |
| [`traefik.md`](traefik.md)   | Docker/Traefik-label setups           | Via resolver    |

## Caddy (recommended)

Already wired into `docker-compose.yml`. Set `TOCK_DOMAIN` and
`TOCK_ACME_EMAIL` in `.env`, point DNS at the host, then:

```sh
docker compose --profile tls up -d
```

Caddy fetches and renews a Let's Encrypt certificate automatically and serves
`https://$TOCK_DOMAIN`.

See [`docs/self-hosting.md`](../../docs/self-hosting.md) for the full guide.
