#!/usr/bin/env python3
"""Build amanfold.com into ../site/ (plain static files, no runtime build).

    python3 website/build.py            # production: clean URLs (/security/)
    python3 website/build.py --preview  # links point at .../index.html, for
                                        # hosts that do not map folders to
                                        # index.html (the review preview)

Words live in i18n.py, styles in assets/site.css, behaviour in
assets/site.js. The output has no inline scripts or styles, no third-party
requests and no cookies, so it runs under a strict CSP:
    default-src 'self'; connect-src 'self' https://api.amanfold.com
"""

import hashlib
import html
import re
import shutil
import sys
from pathlib import Path

from i18n import AR, EN

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
OUT = REPO / "site"
BRAND = REPO / "brand"
SITE = "https://amanfold.com"
API = "https://api.amanfold.com"
GITHUB = "https://github.com/dream1438/amanfold"
REVIEW = GITHUB + "/blob/main/docs/SECURITY.md"
REPORT = GITHUB + "/security/advisories/new"
PREVIEW = "--preview" in sys.argv

# page id -> path inside a language root ("" = home)
PAGES = {
    "home": "",
    "security": "security/",
    "privacy": "privacy/",
    "confirm": "beta/confirm/",
    "leave": "beta/leave/",
}


def lang_root(t):
    return "" if t["lang"] == "en" else t["lang"] + "/"


def href(t, page, frm, anchor=""):
    """Relative link from output file `frm` (path under site/) to `page`."""
    target = lang_root(t) + PAGES[page]
    if PREVIEW:
        target += "index.html"
    return ("../" * frm.count("/") + target or "./") + anchor


def asset(frm, name):
    return "../" * frm.count("/") + "assets/" + name


def icon(frm, name, cls="i"):
    return (
        f'<svg class="{cls}" aria-hidden="true" focusable="false">'
        f'<use href="{asset(frm, "icons.svg")}#af-{name}"></use></svg>'
    )


def items(s):
    return s.split("|")


def fake_cipher(seed, n):
    """Deterministic base64url noise that looks like an encrypted record."""
    out = b""
    i = 0
    while len(out) < n:
        out += hashlib.sha256(f"{seed}:{i}".encode()).digest()
        i += 1
    import base64

    return base64.urlsafe_b64encode(out[:n]).decode().rstrip("=")


# ---------------------------------------------------------------------------
# Layout
# ---------------------------------------------------------------------------


def layout(t, page, frm, title, desc, body, *, robots="index,follow", body_class=""):
    other = AR if t is EN else EN
    canonical = SITE + "/" + lang_root(t) + PAGES[page]
    alt_en = SITE + "/" + PAGES[page]
    alt_ar = SITE + "/ar/" + PAGES[page]
    nav = [
        ("problems", t["nav_problems"], href(t, "home", frm, "#why")),
        ("security", t["nav_security"], href(t, "security", frm)),
        ("pricing", t["nav_pricing"], href(t, "home", frm, "#pricing")),
        ("faq", t["nav_faq"], href(t, "home", frm, "#faq")),
    ]
    nav_html = "".join(
        f'<li><a href="{u}"{" aria-current=\"page\"" if k == page else ""}>{label}</a></li>'
        for k, label, u in nav
    )
    switch = href(other, page, frm)
    return f"""<!doctype html>
<html lang="{t["lang"]}" dir="{t["dir"]}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<title>{html.escape(title)}</title>
<meta name="description" content="{html.escape(desc)}">
<meta name="robots" content="{robots}">
<meta name="af-api" content="{API}">
<meta name="color-scheme" content="light dark">
<meta name="theme-color" content="#F4F6F3" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#081410" media="(prefers-color-scheme: dark)">
<meta name="referrer" content="no-referrer">
<link rel="canonical" href="{canonical}">
<link rel="alternate" hreflang="en" href="{alt_en}">
<link rel="alternate" hreflang="ar" href="{alt_ar}">
<link rel="alternate" hreflang="x-default" href="{alt_en}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="Amanfold">
<meta property="og:title" content="{html.escape(title)}">
<meta property="og:description" content="{html.escape(desc)}">
<meta property="og:url" content="{canonical}">
<meta property="og:image" content="{SITE}/assets/og.png">
<meta property="og:locale" content="{"ar_AR" if t["lang"] == "ar" else "en_US"}">
<link rel="icon" href="{asset(frm, "favicon.ico")}" sizes="48x48">
<link rel="icon" href="{asset(frm, "favicon.svg")}" type="image/svg+xml">
<link rel="apple-touch-icon" href="{asset(frm, "apple-touch-icon.png")}">
<link rel="preload" href="{asset(frm, "fonts/readex-pro-" + ("arabic" if t["lang"] == "ar" else "latin") + "-wght-normal.woff2")}" as="font" type="font/woff2" crossorigin>
<link rel="stylesheet" href="{asset(frm, "site.css")}">
<script src="{asset(frm, "site.js")}" defer></script>
</head>
<body class="{body_class}">
<a class="skip" href="#main">{t["skip"]}</a>
<header class="top">
  <div class="wrap top-in">
    <a class="brand" href="{href(t, "home", frm)}" aria-label="Amanfold">
      <img class="logo logo-light" src="{asset(frm, "logo.svg")}" alt="Amanfold" width="146" height="20">
      <img class="logo logo-dark" src="{asset(frm, "logo-white.svg")}" alt="" width="146" height="20">
    </a>
    <button class="menu-btn" type="button" aria-expanded="false" aria-controls="site-nav"><svg class="i" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h16"/></svg><span>{t["nav_menu"]}</span></button>
    <nav id="site-nav" class="nav" aria-label="{t["nav_menu"]}">
      <ul>{nav_html}</ul>
      <a class="lang" href="{switch}" hreflang="{other["lang"]}" lang="{other["lang"]}">{t["other_lang_label"]}</a>
      <a class="btn btn-sm" href="{href(t, "home", frm, "#join")}">{t["nav_join"]}</a>
    </nav>
  </div>
</header>
<main id="main">
{body}
</main>
<footer class="foot">
  <div class="wrap foot-in">
    <div class="foot-brand">
      <img class="logo logo-light" src="{asset(frm, "logo.svg")}" alt="Amanfold" width="146" height="20">
      <img class="logo logo-dark" src="{asset(frm, "logo-white.svg")}" alt="" width="146" height="20">
      <p>{t["footer_tagline"]}</p>
      <p class="muted small">{t["footer_status"]}</p>
    </div>
    <div>
      <h2 class="foot-h">{t["footer_product"]}</h2>
      <ul>
        <li><a href="{href(t, "home", frm)}">{t["footer_home"]}</a></li>
        <li><a href="{href(t, "home", frm, "#pricing")}">{t["nav_pricing"]}</a></li>
        <li><a href="{href(t, "home", frm, "#join")}">{t["nav_join"]}</a></li>
        <li><a href="{switch}" hreflang="{other["lang"]}" lang="{other["lang"]}">{t["other_lang_label"]}</a></li>
      </ul>
    </div>
    <div>
      <h2 class="foot-h">{t["footer_trust"]}</h2>
      <ul>
        <li><a href="{href(t, "security", frm)}">{t["footer_security"]}</a></li>
        <li><a href="{href(t, "privacy", frm)}">{t["footer_privacy"]}</a></li>
        <li><a href="{GITHUB}" rel="noopener">{t["footer_source"]}</a></li>
        <li><a href="{REPORT}" rel="noopener">{t["footer_report"]}</a></li>
      </ul>
    </div>
  </div>
  <div class="wrap"><p class="muted small copy">{t["footer_copy"]}</p></div>
</footer>
</body>
</html>
"""


# ---------------------------------------------------------------------------
# Home
# ---------------------------------------------------------------------------


def home(t, frm):
    priv = href(t, "privacy", frm)
    problems = "".join(
        f"""<li class="prob">
  <p class="prob-q"><span class="tag tag-muted">{t["label_problem"]}</span>{t[f"p{i}_problem"]}</p>
  <p class="prob-a"><span class="tag">{icon(frm, "done", "i i-sm")}{t["label_fix"]}</span>{t[f"p{i}_fix"]}</p>
</li>"""
        for i in range(1, 9)
    )
    feats = [
        ("login", "passwords"),
        ("card", "cards"),
        ("one-time-code", "codes"),
        ("family", "sharing"),
        ("recovery-kit", "recovery"),
        ("sync", "devices"),
    ]
    features = "".join(
        f"""<li class="feat">{icon(frm, ic, "i i-lg")}<h3>{t[f"f_{k}_h"]}</h3><p>{t[f"f_{k}_p"]}</p></li>"""
        for ic, k in feats
    )
    pillars = "".join(
        f"""<li><h3>{t[f"s{i}_h"]}</h3><p>{t[f"s{i}_p"]}</p></li>""" for i in range(1, 5)
    )
    sees = "".join(f"<li>{x}</li>" for x in items(t["stores_sees_list"]))
    cannot = "".join(f"<li>{x}</li>" for x in items(t["stores_cannot_list"]))

    def plan(key, popular=False):
        pts = "".join(f"<li>{icon(frm, 'done', 'i i-sm')}<span>{p}</span></li>" for p in items(t[f"plan_{key}_points"]))
        badge = f'<span class="plan-badge">{t["plan_popular"]}</span>' if popular else ""
        return f"""<li class="plan{" plan-pop" if popular else ""}">
  <div class="plan-head"><h3>{t[f"plan_{key}"]}</h3>{badge}</div>
  <p class="price"><span class="amount">{t[f"plan_{key}_price"]}</span> <span class="per">{t[f"plan_{key}_per"]}</span></p>
  <ul class="ticks">{pts}</ul>
  <a class="btn btn-ghost" href="#join" data-plan="{key}">{t["plan_cta"]}</a>
</li>"""

    plans = plan("free") + plan("premium") + plan("family", popular=True) + plan("teams")
    faq = "".join(
        f"""<details class="qa"><summary>{t[f"q{i}"]}</summary><p>{t[f"a{i}"]}</p></details>"""
        for i in range(1, 7)
    )
    plan_opts = "".join(
        f"""<label class="chip"><input type="radio" name="plan" value="{k}"{" checked" if k == "free" else ""}><span>{t[f"plan_{k}"]}</span></label>"""
        for k in ["free", "premium", "family", "teams"]
    )
    c1 = fake_cipher("bank", 66)
    c2 = fake_cipher("card", 66)
    msgs = " ".join(
        f'data-msg-{k}="{html.escape(t["join_" + k.replace("-", "_")])}"'
        for k in ["err-email", "err-rate", "err-net", "err-other", "sending", "submit"]
    )
    body = f"""
<section class="hero">
  <div class="wrap hero-in">
    <div class="hero-copy">
      <p class="badge"><span class="dot" aria-hidden="true"></span>{t["beta_badge"]}</p>
      <h1>{t["hero_h1"]}</h1>
      <p class="lead">{t["hero_lead"]}</p>
      <div class="cta-row">
        <a class="btn" href="#join">{t["hero_cta"]}</a>
        <a class="btn btn-ghost" href="{href(t, "security", frm)}">{t["hero_cta2"]}</a>
      </div>
      <p class="muted small">{t["hero_note"]}</p>
    </div>
    <figure class="demo" aria-label="{t["demo_label"]}">
      <div class="vault folded">
        <p class="vault-h">{icon(frm, "locked", "i i-sm")}<span>{t["demo_vault"]}</span><span class="muted small">{t["demo_label"]}</span></p>
        <ul class="vault-list">
          <li class="row">
            <span class="row-ic">{icon(frm, "login")}</span>
            <span class="row-t"><strong>{t["demo_bank"]}</strong><span class="muted small" dir="ltr">{t["demo_bank_user"]}</span></span>
            <span class="mono dots" aria-hidden="true">••••••••••••</span>
          </li>
          <li class="row">
            <span class="row-ic">{icon(frm, "card")}</span>
            <span class="row-t"><strong>{t["demo_card"]}</strong><span class="muted small">{t["demo_card_meta"]}</span></span>
            <span class="mono" dir="ltr">•••• 4821</span>
          </li>
          <li class="row">
            <span class="row-ic">{icon(frm, "one-time-code")}</span>
            <span class="row-t"><strong>{t["demo_code"]}</strong><span class="muted small">{t["demo_code_meta"]}</span></span>
            <span class="mono code" dir="ltr">482 913</span>
          </li>
        </ul>
        <p class="warn">{icon(frm, "lookalike", "i i-sm")}<span>{t["demo_lookalike"]}</span></p>
      </div>
      <figcaption class="wire">
        <p class="wire-h">{icon(frm, "sync", "i i-sm")}<span>{t["demo_server"]}</span></p>
        <p class="mono cipher" dir="ltr">{c1}<br>{c2}</p>
        <p class="muted small">{t["demo_server_note"]}</p>
      </figcaption>
    </figure>
  </div>
</section>

<section class="beta-note" aria-labelledby="beta-h">
  <div class="wrap beta-in">
    {icon(frm, "warning", "i i-lg")}
    <div>
      <h2 id="beta-h">{t["beta_h"]}</h2>
      <p>{t["beta_p1"]}</p>
      <p>{t["beta_p2"]} <a href="{REVIEW}" rel="noopener">{t["beta_link"]}</a></p>
    </div>
  </div>
</section>

<section id="why" class="sec">
  <div class="wrap">
    <p class="eyebrow">{t["problems_eyebrow"]}</p>
    <h2 class="h2">{t["problems_h"]}</h2>
    <p class="sec-lead">{t["problems_lead"]}</p>
    <ul class="probs">{problems}</ul>
  </div>
</section>

<section class="sec sec-alt">
  <div class="wrap">
    <p class="eyebrow">{t["features_eyebrow"]}</p>
    <h2 class="h2">{t["features_h"]}</h2>
    <ul class="feats">{features}</ul>
  </div>
</section>

<section id="security" class="sec">
  <div class="wrap sec-grid">
    <div>
      <p class="eyebrow">{t["security_eyebrow"]}</p>
      <h2 class="h2">{t["security_h"]}</h2>
      <p class="sec-lead">{t["security_lead"]}</p>
      <ul class="pillars">{pillars}</ul>
      <p><a class="btn btn-ghost" href="{href(t, "security", frm)}">{t["security_more"]}</a></p>
    </div>
    <aside class="stores folded" aria-labelledby="stores-h">
      <h3 id="stores-h">{t["stores_h"]}</h3>
      <p class="stores-k">{icon(frm, "show", "i i-sm")}<span>{t["stores_sees"]}</span></p>
      <ul>{sees}</ul>
      <p class="stores-k stores-no">{icon(frm, "hide", "i i-sm")}<span>{t["stores_cannot"]}</span></p>
      <ul class="no">{cannot}</ul>
    </aside>
  </div>
</section>

<section id="pricing" class="sec sec-alt">
  <div class="wrap">
    <p class="eyebrow">{t["pricing_eyebrow"]}</p>
    <h2 class="h2">{t["pricing_h"]}</h2>
    <p class="sec-lead">{t["pricing_lead"]}</p>
    <ul class="plans">{plans}</ul>
    <p class="muted small">{t["pricing_note"]}</p>
  </div>
</section>

<section id="faq" class="sec">
  <div class="wrap narrow">
    <p class="eyebrow">{t["faq_eyebrow"]}</p>
    <h2 class="h2">{t["faq_h"]}</h2>
    <div class="faq">{faq}</div>
  </div>
</section>

<section id="join" class="sec join">
  <div class="wrap join-in">
    <div>
      <p class="eyebrow">{t["join_eyebrow"]}</p>
      <h2 class="h2">{t["join_h"]}</h2>
      <p class="sec-lead">{t["join_lead"]}</p>
    </div>
    <div class="join-card folded">
      <form id="join-form" class="join-form" novalidate data-lang="{t["lang"]}" {msgs}>
        <label for="join-email" class="lbl">{t["join_email"]}</label>
        <input id="join-email" name="email" type="email" inputmode="email" autocomplete="email" dir="ltr" required maxlength="254" placeholder="{t["join_email_ph"]}" aria-describedby="join-error">
        <fieldset class="plans-pick">
          <legend class="lbl">{t["join_plan"]}</legend>
          <p class="muted small">{t["join_plan_hint"]}</p>
          <div class="chips">{plan_opts}</div>
        </fieldset>
        <p id="join-error" class="err" role="alert" hidden></p>
        <button class="btn btn-wide" type="submit">{t["join_submit"]}</button>
        <p class="muted small">{t["join_privacy"].format(privacy=priv)}</p>
      </form>
      <div id="join-done" class="join-done" hidden tabindex="-1">
        {icon(frm, "done", "i i-lg")}
        <h3>{t["join_ok_h"]}</h3>
        <p>{t["join_ok_p"]}</p>
      </div>
    </div>
  </div>
</section>
"""
    return layout(t, "home", frm, t["home_title"], t["home_desc"], body, body_class="home")


# ---------------------------------------------------------------------------
# Security
# ---------------------------------------------------------------------------


def security(t, frm):
    secs = []
    toc = []
    for i in range(1, 11):
        sid = f"s{i}"
        paras = f"<p>{t[f'sec_{i}_p']}</p>"
        if f"sec_{i}_p2" in t:
            paras += f"<p>{t[f'sec_{i}_p2']}</p>"
        extra = ""
        if i == 9:
            extra = f'<p><a class="btn btn-ghost" href="{REVIEW}" rel="noopener">{t["sec_review"]}</a></p>'
        if i == 10:
            extra = f'<p><a class="btn btn-ghost" href="{REPORT}" rel="noopener">{t["sec_report"]}</a></p>'
        cls = ' class="callout"' if i == 9 else ""
        secs.append(f'<section id="{sid}"{cls}><h2>{t[f"sec_{i}_h"]}</h2>{paras}{extra}</section>')
        toc.append(f'<li><a href="#{sid}">{t[f"sec_{i}_h"]}</a></li>')
    sees = "".join(f"<li>{x}</li>" for x in items(t["stores_sees_list"]))
    cannot = "".join(f"<li>{x}</li>" for x in items(t["stores_cannot_list"]))
    body = f"""
<section class="page-head">
  <div class="wrap narrow">
    <p class="badge"><span class="dot" aria-hidden="true"></span>{t["beta_badge"]}</p>
    <h1>{t["sec_h1"]}</h1>
    <p class="lead">{t["sec_lead"]}</p>
  </div>
</section>
<div class="wrap doc-grid">
  <nav class="toc" aria-labelledby="toc-h">
    <h2 id="toc-h" class="toc-h">{t["sec_toc"]}</h2>
    <ol>{"".join(toc)}</ol>
  </nav>
  <article class="doc">
    {secs[0]}
    <aside class="stores stores-inline folded" aria-labelledby="stores-h">
      <h3 id="stores-h">{t["stores_h"]}</h3>
      <div class="stores-cols">
        <div><p class="stores-k">{icon(frm, "show", "i i-sm")}<span>{t["stores_sees"]}</span></p><ul>{sees}</ul></div>
        <div><p class="stores-k stores-no">{icon(frm, "hide", "i i-sm")}<span>{t["stores_cannot"]}</span></p><ul class="no">{cannot}</ul></div>
      </div>
    </aside>
    {"".join(secs[1:])}
  </article>
</div>
"""
    return layout(t, "security", frm, t["sec_title"] + " · Amanfold", t["sec_desc"], body)


# ---------------------------------------------------------------------------
# Privacy, confirm, leave, 404
# ---------------------------------------------------------------------------


def privacy(t, frm):
    parts = []
    for i in range(1, 7):
        p = f"<p>{t[f'priv_{i}_p']}</p>"
        if i == 2:
            p += "<ul class=\"bul\">" + "".join(f"<li>{x}</li>" for x in items(t["priv_2_list"])) + "</ul>"
            p += f"<p>{t['priv_2_p2']}</p>"
        parts.append(f"<section><h2>{t[f'priv_{i}_h']}</h2>{p}</section>")
    body = f"""
<section class="page-head">
  <div class="wrap narrow">
    <h1>{t["priv_h1"]}</h1>
    <p class="muted small">{t["priv_updated"]}</p>
    <p class="lead">{t["priv_lead"]} <a href="{href(t, "security", frm)}">{t["read_security"]}</a></p>
  </div>
</section>
<div class="wrap narrow"><article class="doc">{"".join(parts)}</article></div>
"""
    return layout(t, "privacy", frm, t["priv_title"] + " · Amanfold", t["priv_desc"], body)


def status_page(t, frm, page, title, inner, action):
    body = f"""
<section class="status">
  <div class="wrap narrow">
    <div class="status-card folded" id="status" data-action="{action}">
      {inner}
    </div>
  </div>
</section>
"""
    return layout(t, page, frm, title + " · Amanfold", title, body, robots="noindex,nofollow")


def state(name, ic, h, p, links, frm, hidden=True):
    return f"""<div class="state" data-state="{name}"{" hidden" if hidden else ""}>
  {icon(frm, ic, "i i-lg") if ic else '<span class="spinner" aria-hidden="true"></span>'}
  <h1 tabindex="-1">{h}</h1>
  {f"<p>{p}</p>" if p else ""}
  {f'<p class="cta-row">{links}</p>' if links else ""}
</div>"""


def confirm(t, frm):
    home_l = f'<a class="btn btn-ghost" href="{href(t, "home", frm)}">{t["back_home"]}</a>'
    again = f'<a class="btn" href="{href(t, "home", frm, "#join")}">{t["join_again"]}</a>'
    sec = f'<a class="btn" href="{href(t, "security", frm)}">{t["read_security"]}</a>'
    inner = "".join(
        [
            state("working", None, t["conf_working"], "", "", frm, hidden=False),
            state("ok", "done", t["conf_ok_h"], t["conf_ok_p"], sec + home_l, frm),
            state("bad", "warning", t["conf_bad_h"], t["conf_bad_p"], again, frm),
            state("missing", "warning", t["conf_missing_h"], t["conf_missing_p"], home_l, frm),
            state("error", "warning", t["join_err_other"], t["conf_err_p"], home_l, frm),
        ]
    )
    return status_page(t, frm, "confirm", t["conf_title"], inner, "confirm")


def leave(t, frm):
    home_l = f'<a class="btn btn-ghost" href="{href(t, "home", frm)}">{t["back_home"]}</a>'
    btn = f'<button class="btn btn-danger" type="button" data-leave>{t["leave_btn"]}</button>'
    inner = "".join(
        [
            state("ask", "delete", t["leave_h"], t["leave_p"], btn + home_l, frm, hidden=False),
            state("working", None, t["join_sending"], "", "", frm),
            state("ok", "done", t["leave_ok_h"], t["leave_ok_p"], home_l, frm),
            state("missing", "warning", t["conf_missing_h"], t["conf_missing_p"], home_l, frm),
            state("error", "warning", t["join_err_other"], t["conf_err_p"], home_l, frm),
        ]
    )
    return status_page(t, frm, "leave", t["leave_title"], inner, "leave")


def not_found():
    # Served for any missing path, so all links are absolute from the root.
    t = EN
    body = f"""
<section class="status">
  <div class="wrap narrow"><div class="status-card"><div class="state">
    {'<svg class="i i-lg" aria-hidden="true"><use href="/assets/icons.svg#af-search"></use></svg>'}
    <h1>{t["nf_h"]}</h1><p>{t["nf_p"]}</p>
    <p class="cta-row"><a class="btn" href="/">{t["back_home"]}</a><a class="btn btn-ghost" href="/ar/" lang="ar">{AR["back_home"]}</a></p>
  </div></div></div>
</section>"""
    page = layout(t, "home", "404.html", t["nf_title"] + " · Amanfold", t["nf_p"], body, robots="noindex")
    # make every relative asset/link absolute
    return re.sub(r'(href|src)="(?!https?:|/|#)([^"]*)"', lambda m: f'{m.group(1)}="/{m.group(2)}"', page)


# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------


def write(rel, text):
    p = OUT / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


def main():
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir()
    builders = {"home": home, "security": security, "privacy": privacy, "confirm": confirm, "leave": leave}
    for t in (EN, AR):
        for page, path in PAGES.items():
            rel = lang_root(t) + path + "index.html"
            write(rel, builders[page](t, rel))
    write("404.html", not_found())

    a = OUT / "assets"
    a.mkdir(exist_ok=True)
    shutil.copy(HERE / "assets" / "site.css", a / "site.css")
    shutil.copy(HERE / "assets" / "site.js", a / "site.js")
    shutil.copytree(HERE / "fonts", a / "fonts")
    shutil.copy(BRAND / "logo" / "amanfold-logo.svg", a / "logo.svg")
    shutil.copy(BRAND / "logo" / "amanfold-logo-white.svg", a / "logo-white.svg")
    # The brand favicon switches colour with an inline <style>, which the
    # site's CSP (style-src 'self') would block; use plain fills instead.
    fav = (BRAND / "app-icon" / "favicon.svg").read_text()
    fav = re.sub(r"<style>.*?</style>", "", fav).replace("<path ", '<path fill="#0B6E4F" ', 1)
    (a / "favicon.svg").write_text(fav)
    shutil.copy(BRAND / "app-icon" / "favicon.ico", a / "favicon.ico")
    shutil.copy(BRAND / "app-icon" / "png" / "app-icon-180.png", a / "apple-touch-icon.png")
    if (HERE / "assets" / "og.png").exists():
        shutil.copy(HERE / "assets" / "og.png", a / "og.png")
    # The sprite hides itself with an inline style attribute, which a strict
    # CSP (no 'unsafe-inline' styles) reports. It is only used via <use>.
    sprite = (BRAND / "icons" / "sprite.svg").read_text()
    (a / "icons.svg").write_text(sprite.replace(' style="display:none"', ""))
    # tokens.css is the brand's single source of colour, type and spacing.
    tokens = (BRAND / "tokens" / "tokens.css").read_text()
    (a / "site.css").write_text(tokens + "\n" + (a / "site.css").read_text())

    urls = "".join(
        f"<url><loc>{SITE}/{lang_root(t)}{PAGES[p]}</loc></url>"
        for t in (EN, AR)
        for p in ("home", "security", "privacy")
    )
    write(
        "sitemap.xml",
        f'<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{urls}</urlset>\n',
    )
    write("robots.txt", f"User-agent: *\nDisallow: /beta/\nDisallow: /ar/beta/\nSitemap: {SITE}/sitemap.xml\n")
    write(
        ".well-known/security.txt",
        f"Contact: {REPORT}\nPolicy: {SITE}/security/#s10\nPreferred-Languages: en, ar\n"
        "Expires: 2027-11-28T00:00:00Z\nCanonical: https://amanfold.com/.well-known/security.txt\n",
    )
    print("built", sum(1 for _ in OUT.rglob("*") if _.is_file()), "files into", OUT, "(preview)" if PREVIEW else "")


if __name__ == "__main__":
    main()
