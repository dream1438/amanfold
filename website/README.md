# amanfold.com

The public website and beta waitlist, in English and Arabic (right to left).

- `i18n.py` holds every word on the site, one dictionary per language with the same keys. Edit copy here.
- `build.py` writes the finished site into `../site/`, which is committed and served as plain files (Caddy, see `deploy/`). No build step runs on the server.
- `assets/site.css` and `assets/site.js` are copied as they are; `brand/tokens/tokens.css` is put in front of the CSS so colours, type and spacing come from the brand.
- `fonts/` has self-hosted WOFF2 subsets of Readex Pro and IBM Plex (SIL OFL, see `fonts/OFL.txt`).

```sh
python3 website/build.py            # rebuild site/ after any change, then commit both
python3 website/build.py --preview  # links to .../index.html, for hosts without folder indexes
```

CI rebuilds the site and fails if `site/` is out of date.

## Privacy and security rules

- No cookies, no analytics, no third-party requests of any kind (fonts included).
- No inline scripts or styles, so the site runs under the strict CSP in `deploy/Caddyfile`:
  `default-src 'self'; connect-src 'self' https://api.amanfold.com`.
- The waitlist is double opt-in through the Amanfold server (`POST /v1/waitlist`, see `docs/SERVER.md`). The confirm and leave links carry their token after `#`, so it never reaches a web server log, and the page removes it from the address bar once read. Confirming needs a click-free script POST, so mail scanners that open links do not confirm sign-ups by themselves.
- The API address is the `af-api` meta tag (`API` in `build.py`).

## Pages

| Path | Arabic | |
| --- | --- | --- |
| `/` | `/ar/` | Home: problems we fix, features, security summary, pricing, questions, waitlist |
| `/security/` | `/ar/security/` | Plain-language security explainer, audit status, how to report issues |
| `/privacy/` | `/ar/privacy/` | Privacy notice for the site and waitlist |
| `/beta/confirm/` | `/ar/beta/confirm/` | Opened from the waitlist email |
| `/beta/leave/` | `/ar/beta/leave/` | Leave the waitlist |

Every page links to its twin in the other language, with `hreflang` alternates for search engines. The English logo is used in both languages, per the brand rules.
