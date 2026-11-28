"""Generate the Amanfold brand guide (brand/guide/index.html) from the same sources as the assets.
Run from the repository root after build.py and icons.py."""
from pathlib import Path
import html as H

from build import _flaps, RING_CLIP, SEAL, words, fmt
from palette import OASIS, SAFFRON, MIST, SEMANTIC
from icons import ICONS, el

ROOT = Path(__file__).resolve().parent.parent
_uid = [0]


def mk(cls="", label=None):
    """Inline mark whose colours come from CSS custom properties (theme-aware)."""
    _uid[0] += 1
    u = f"g{_uid[0]}"
    F = _flaps()
    cx, cy, r = SEAL
    a = f'role="img" aria-label="{label}"' if label else 'aria-hidden="true"'
    return (f'<svg class="mk {cls}" viewBox="0 0 64 64" {a}><clipPath id="{u}"><path clip-rule="evenodd" d="{RING_CLIP}"/></clipPath>'
            f'<g clip-path="url(#{u})"><path class="fl t" d="{F["top"]}"/><path class="fd r" d="{F["right"]}"/>'
            f'<path class="fl b" d="{F["bottom"]}"/><path class="fd l" d="{F["left"]}"/></g>'
            f'<circle class="seal" cx="{cx}" cy="{cy}" r="{r}"/></svg>')


def icon(name, cls="ic"):
    return f'<svg class="{cls}" viewBox="0 0 24 24" aria-hidden="true">{"".join(el(p) for p in ICONS[name])}</svg>'


LAT, ARA = words()


def wordpath(word, height_cap, cls="wm"):
    s = height_cap / word["cap"]
    x0, top, x1, bottom = (v * s for v in word["bbox"])
    w, h = x1 - x0, bottom - top
    return (f'<svg class="{cls}" viewBox="0 0 {fmt(w)} {fmt(h)}" style="aspect-ratio:{fmt(w)}/{fmt(h)}" aria-hidden="true">'
            f'<path transform="translate({fmt(-x0)} {fmt(-top)}) scale({s:.4f})" d="{word["d"]}"/></svg>')


def lockup(rtl=False, cls=""):
    word = ARA if rtl else LAT
    return (f'<div class="lockup {cls}" dir="{"rtl" if rtl else "ltr"}" role="img" aria-label="{"أمانفولد" if rtl else "Amanfold"}">'
            f'{mk()}{wordpath(word, 100)}</div>')


def swatches(name, scale, brand_step, note):
    cells = []
    for k, v in scale.items():
        dark_text = MIST[0] if (int(v[1:3], 16) * .299 + int(v[3:5], 16) * .587 + int(v[5:7], 16) * .114) < 150 else MIST[950]
        cls = ' class="brand" title="Brand colour"' if k == brand_step else ""
        cells.append(f'<li{cls} style="background:{v};color:{dark_text}"><b>{k}</b><code>{v}</code></li>')
    return (f'<div class="scale"><div class="scale-h"><h4>{name}</h4><p>{note}</p></div>'
            f'<ol class="chips">{"".join(cells)}</ol></div>')


def section(id_, num, title, intro, body):
    return (f'<section id="{id_}" class="sec"><header class="sec-h"><p class="num">{num}</p><h2>{title}</h2>'
            f'<p class="lede">{intro}</p></header><div class="sec-b">{body}</div></section>')


def page():
    icons_grid = "".join(f'<li>{icon(n)}<span>{n}</span></li>' for n in ICONS)

    hero = f'''
<section class="hero">
  <div class="hero-in">
    <div class="hero-mark" id="heroMark">{mk("big", "Amanfold mark")}</div>
    <div class="hero-copy">
      <p class="kicker">Brand guide · version 1.0 · November 2026</p>
      <h1>{wordpath(LAT, 100, "wm hero-wm")}<span class="sr">Amanfold</span></h1>
      <p class="tagline">Everything you keep, safely folded.</p>
      <p class="tagline ar" lang="ar" dir="rtl">كل ما تحفظه، مطويٌّ بأمان.</p>
      <button class="ghost" id="replay" type="button">{icon("sync")} Fold it again</button>
    </div>
  </div>
</section>'''

    idea = section("idea", "01", "The idea",
        "Amanfold joins two words. <em>Aman</em> (أمان) is Arabic for safety, trust and peace of mind. <em>Fold</em> is what you do to keep something private.",
        f'''<div class="two">
  <div class="prose">
    <p>Before envelopes existed, people sealed letters by folding them in on themselves and closing the last flap with wax or a paper lock. If anyone opened the letter on the way, the folds showed it. Historians call this <strong>letterlocking</strong>. It was everyday, end-to-end privacy, centuries before computers.</p>
    <p>The Amanfold mark is that letter: a square sheet with its four corners folded to the centre, held shut by a saffron seal. It says what the product does without a padlock or a shield. Your passwords, cards and notes are folded shut on your own device, and only you hold the seal.</p>
    <p>The same idea runs through the brand. Light and dark faces show the two sides of each fold. Containers in the interface carry one folded corner. Saffron appears once per screen, like a seal.</p>
  </div>
  <figure class="anatomy">
    {mk("anat")}
    <figcaption>
      <ol>
        <li><b>Four flaps.</b> The corners of one sheet, folded in. Light faces catch the light; dark faces turn away.</li>
        <li><b>The fold lines.</b> A clean gap between flaps, so the mark reads even in one colour.</li>
        <li><b>The seal.</b> Saffron, set apart by a ring. The one warm note in the brand.</li>
      </ol>
    </figcaption>
  </figure>
</div>''')

    logo = section("logo", "02", "Logo",
        "Use the full lockup wherever there is room, and the mark alone where the name is already present, such as an app icon or a browser toolbar.",
        f'''<div class="logo-grid">
  <figure class="tile light">{lockup()}<figcaption>Primary lockup · <code>amanfold-logo.svg</code></figcaption></figure>
  <figure class="tile light">{lockup(rtl=True)}<figcaption>Arabic lockup, mark on the right · <code>amanfold-logo-arabic.svg</code></figcaption></figure>
  <figure class="tile dark">{lockup(cls="rev")}<figcaption>Reversed, on Oasis 800 · <code>amanfold-logo-reversed.svg</code></figcaption></figure>
  <figure class="tile light stack">{mk()}<span class="stk-lat">{wordpath(LAT, 100)}</span><span class="stk-ar">{wordpath(ARA, 88)}</span><figcaption>Stacked, for splash screens and stickers · <code>amanfold-logo-stacked.svg</code></figcaption></figure>
</div>
<div class="rules">
  <figure class="rule">
    <div class="clear">{lockup()}<span class="cz t"></span><span class="cz b"></span><span class="cz l"></span><span class="cz r"></span></div>
    <figcaption><b>Clear space</b> equals the seal's diameter on every side. Keep text, edges and other logos outside it.</figcaption>
  </figure>
  <figure class="rule">
    <div class="minsz"><span>{mk()}<small>16 px</small></span><span>{lockup()}<small>96 px wide</small></span></div>
    <figcaption><b>Minimum size.</b> The mark holds up at 16 px. Use the lockup only from 96 px wide; below that, use the mark alone.</figcaption>
  </figure>
</div>
<h3 class="sub">Please don't</h3>
<ul class="donts">
  <li><div class="dz">{mk("x-rot")}</div><span>Rotate it into a diamond. It stops reading as a folded letter.</span></li>
  <li><div class="dz">{mk("x-col")}</div><span>Recolour the flaps or the seal outside the palette.</span></li>
  <li><div class="dz">{mk("x-str")}</div><span>Stretch, squash or skew it.</span></li>
  <li><div class="dz x-lock">{mk()}<svg viewBox="0 0 24 24" class="pad" aria-hidden="true"><path d="M7 11V8a5 5 0 0 1 10 0v3"/><rect x="5" y="11" width="14" height="10" rx="2"/></svg></div><span>Add padlocks, shields or keys. The fold already says it.</span></li>
</ul>''')

    appicon = section("icon", "03", "App icon",
        "One icon for every platform: the mark in its reversed colours on an Oasis 800 tile. Stores and launchers apply their own corner masks, so the source is a full-bleed square.",
        f'''<div class="icon-row">
  <div class="phone-row">
    <div class="homescreen">
      <span class="app"><i class="ph p1"></i><small>Photos</small></span>
      <span class="app"><i class="ph p2"></i><small>Mail</small></span>
      <span class="app"><span class="appicon">{mk("rev")}</span><small>Amanfold</small></span>
      <span class="app"><i class="ph p3"></i><small>Maps</small></span>
    </div>
    <p class="cap">On a home screen beside other apps. The saffron seal is the brightest point in the row.</p>
  </div>
  <div class="sizes">
    <span class="appicon s120">{mk("rev")}</span><span class="appicon s64">{mk("rev")}</span><span class="appicon s40">{mk("rev")}</span>
    <span class="appicon s24">{mk("rev")}</span><span class="appicon s16">{mk("rev")}</span>
  </div>
  <div class="tabbar"><span class="tab">{'<span class="appicon s16">' + mk("rev") + '</span>'}Amanfold · Your vault</span><span class="tab dim">Inbox</span><span class="tab dim">Docs</span></div>
</div>
<p class="note">Files: <code>app-icon/app-icon-1024.svg</code> (iOS, macOS, Windows, stores), <code>android-adaptive-foreground.svg</code> + <code>android-adaptive-background.svg</code> + <code>android-monochrome.svg</code> (Android 13 themed icons), <code>favicon.svg</code> and <code>favicon.ico</code>, plus PNGs from 16 to 1024 px in <code>app-icon/png/</code>.</p>''')

    colour = section("colour", "04", "Colour",
        "Deep Oasis green carries the brand: calm, natural, and unlike the blues most security apps use. Saffron is the seal, used sparingly. Mist neutrals are tinted towards Oasis so greys never look accidental.",
        f'''{swatches("Oasis", OASIS, 800, "Primary. Oasis 800 (outlined) is the brand colour, used for the app icon, primary buttons and headings on light.")}
{swatches("Saffron", SAFFRON, 500, "The seal. Saffron 500 (outlined) is the seal colour. Never use Saffron 500 for text on light backgrounds; use Saffron 700 instead.")}
{swatches("Mist", MIST, None, "Neutrals for text, surfaces and borders, with a slight green bias.")}
<div class="sem">
  <h4>Status colours</h4>
  <ul>{"".join(f'<li><span class="dot" style="--c:{v["light"]};--cd:{v["dark"]}"></span><b>{k.title()}</b><code>{v["light"]} / {v["dark"]}</code></li>' for k, v in SEMANTIC.items())}</ul>
  <p>Light / dark theme values. Status colours always come with an icon and words, never colour alone.</p>
</div>
<div class="seal-rule">
  <div class="srv">{mk()}<span class="pill-s">Saved and sealed</span></div>
  <div><h4>One seal per screen</h4><p>Apart from the seal in the logo itself, saffron appears at most once per screen, on its single most reassuring moment: a "sealed" confirmation, the folded corner of a payment card, the main button on a marketing page. If two things on a screen are saffron, one of them is wrong.</p></div>
</div>
<table class="contrast"><caption>Text contrast, checked against WCAG 2.2</caption>
<thead><tr><th>Pairing</th><th>Light theme</th><th>Dark theme</th></tr></thead>
<tbody>
<tr><td>Body text on background</td><td>15.6 : 1</td><td>15.6 : 1</td></tr>
<tr><td>Muted text on surface</td><td>6.5 : 1</td><td>7.4 : 1</td></tr>
<tr><td>Text on primary button</td><td>12.3 : 1</td><td>8.3 : 1</td></tr>
<tr><td>Ink text on saffron</td><td>8.4 : 1</td><td>9.4 : 1</td></tr>
<tr><td>Danger text on surface</td><td>5.7 : 1</td><td>6.8 : 1</td></tr>
</tbody></table>''')

    type_ = section("type", "05", "Typography",
        "Three open-source families (SIL Open Font License) that cover Latin and Arabic with matching rhythm, so English and Arabic screens feel like the same product.",
        '''<div class="faces">
  <article class="face">
    <p class="role">Display · headlines, the wordmark</p>
    <p class="spec d">Readex Pro</p>
    <p class="pair d">Safely folded. <span lang="ar" dir="rtl">مطويٌّ بأمان.</span></p>
    <p class="meta">Weights 500 and 600. One family designed for both scripts, so mixed headlines keep one rhythm.</p>
  </article>
  <article class="face">
    <p class="role">Text · interface and reading</p>
    <p class="spec t">IBM Plex Sans + Plex Sans Arabic</p>
    <p class="pair t">Your vault is encrypted on this device before it syncs.<br><span lang="ar" dir="rtl">تُشفَّر خزنتك على هذا الجهاز قبل المزامنة.</span></p>
    <p class="meta">Weights 400, 500 and 600. Built for screens, with clear figures and a generous Arabic x-height.</p>
  </article>
  <article class="face">
    <p class="role">Secrets · passwords, codes, card numbers</p>
    <p class="spec m">IBM Plex Mono</p>
    <p class="pw" aria-label="Example password">k<span class="n">7</span>Rv<span class="y">#</span>q<span class="n">2</span>Lm<span class="y">!</span>W<span class="n">0</span>o<span class="y">-</span>fs<span class="n">9</span>T</p>
    <p class="meta">Slashed zero and distinct l, 1 and I. Digits show in Saffron 700, symbols in Oasis 500, so a password can be read aloud without mistakes. Passwords are always left-to-right, even in Arabic.</p>
  </article>
</div>
<div class="scale-t" aria-label="Type scale">
  <div><span class="sz">48</span><span class="ts48">Everything you keep</span></div>
  <div><span class="sz">36</span><span class="ts36">Safely folded</span></div>
  <div><span class="sz">28</span><span class="ts28">Your vault</span></div>
  <div><span class="sz">22</span><span class="ts22">Logins and cards</span></div>
  <div><span class="sz">18</span><span class="ts18">Sync is on for 3 devices</span></div>
  <div><span class="sz">16</span><span class="ts16">Body text and most of the interface</span></div>
  <div><span class="sz">14</span><span class="ts14">Secondary details and labels</span></div>
  <div><span class="sz">12</span><span class="ts12">CAPTIONS AND BADGES</span></div>
</div>
<p class="note">Arabic text runs one step larger than Latin at sizes under 16 px, and line height is 1.6 for Arabic body text against 1.5 for Latin. Use Arabic-Indic or Western digits according to the user's locale setting, never mixed in one string.</p>''')

    voice = section("voice", "06", "Voice and tone",
        "Amanfold sounds like a calm, knowledgeable friend who happens to understand cryptography. It never tries to scare anyone into buying.",
        '''<ul class="principles">
  <li><h4>Calm, not alarming</h4><p>State what happened, then the next step. Security news is stressful enough without us adding to it.</p></li>
  <li><h4>Plain, not clever</h4><p>Say "password", not "credential". Explain how encryption works in one sentence when asked, and skip it when not.</p></li>
  <li><h4>Honest about limits</h4><p>Say what we cannot do. We cannot see your vault, so we cannot reset your master password. We are in beta, and we say so.</p></li>
  <li><h4>Equal in every language</h4><p>Arabic and every other language are written by people who think in them, not translated word for word. The same warmth in each.</p></li>
</ul>
<div class="dodont">
  <div class="col do"><h4>We say</h4>
    <p>"Your vault is encrypted on your device. We never see what is inside."</p>
    <p>"This password appeared in a public leak. Change it on example.com, then update it here."</p>
    <p>"Free on every device you own. Premium adds secure file storage and emergency access."</p>
  </div>
  <div class="col dont"><h4>We don't say</h4>
    <p>"Military-grade, unhackable, bank-level security."</p>
    <p>"WARNING! You are at risk! Hackers could steal everything!"</p>
    <p>"Upgrade now or lose your data."</p>
  </div>
</div>
<h3 class="sub">Sample lines, English and Arabic</h3>
<div class="lines">
  <div class="line"><p class="ctx">Empty vault</p><p>Nothing here yet. Add your first login, or bring everything over from another password manager.</p><p lang="ar" dir="rtl">لا شيء هنا بعد. أضف أول حساب، أو انقل كل بياناتك من مدير كلمات مرور آخر.</p></div>
  <div class="line"><p class="ctx">Lookalike site</p><p>This site is paypa1.com, not paypal.com. We did not fill your password.</p><p lang="ar" dir="rtl">هذا الموقع هو paypa1.com وليس paypal.com، لذلك لم نُدخل كلمة مرورك.</p></div>
  <div class="line"><p class="ctx">Recovery kit</p><p>Print your recovery kit and keep it somewhere safe. It is the only way back in if you forget your master password, because we cannot reset it for you.</p><p lang="ar" dir="rtl">اطبع مجموعة الاسترداد واحفظها في مكان آمن. إنها طريقك الوحيد للعودة إذا نسيت كلمة المرور الرئيسية، لأننا لا نستطيع إعادة تعيينها نيابةً عنك.</p></div>
  <div class="line"><p class="ctx">Beta label</p><p>Amanfold is in public beta. Our code is open and reviewed; an independent audit comes next.</p><p lang="ar" dir="rtl">أمانفولد في مرحلة تجريبية عامة. شيفرتنا مفتوحة وخضعت للمراجعة، والتدقيق المستقل هو الخطوة التالية.</p></div>
</div>
<div class="taglines">
  <h4>Tagline</h4>
  <p class="tg">Everything you keep, safely folded.<span lang="ar" dir="rtl">كل ما تحفظه، مطويٌّ بأمان.</span></p>
  <p class="note">Short form for small spaces: "Safely folded." / «مطويٌّ بأمان». Alternatives we considered: "Keep it folded." and "Private by fold."</p>
</div>''')

    iconog = section("icons", "07", "Icons",
        "Twenty-four interface icons drawn on a 24 unit grid with a 1.75 stroke and round ends. Containers such as cards, notes and identities carry the brand's folded corner.",
        f'''<ul class="icons">{icons_grid}</ul>
<p class="note">Icons use the text colour by default and Oasis 800 when active. Arrows and directional icons mirror in right-to-left layouts; everything else, including the sync arrows and the clock, stays as drawn. Files: <code>icons/*.svg</code> and <code>icons/sprite.svg</code>.</p>''')

    ui = section("ui", "08", "Interface basics",
        "Quiet surfaces, generous spacing and one clear action per view. Corners are soft but not round; spacing follows a 4 px grid.",
        f'''<div class="ui-grid">
  <div class="ui-card">
    <h4>Buttons</h4>
    <div class="btns"><button class="b primary" type="button">Save login</button><button class="b secondary" type="button">Cancel</button><button class="b quiet" type="button">{icon("copy")} Copy</button></div>
    <div class="btns"><button class="b seal" type="button">{icon("sealed")} Get Amanfold free</button><span class="cap">Saffron button: marketing pages only, once per page.</span></div>
  </div>
  <div class="ui-card">
    <h4>Fields</h4>
    <label class="fld" for="demoUser"><span>Username</span><input id="demoUser" value="eslam@example.com"></label>
    <label class="fld" for="demoPw"><span>Password</span><span class="pwrow"><input id="demoPw" class="mono" value="k7Rv#q2Lm!W0o-fs9T" spellcheck="false"><button class="iconbtn" type="button" aria-label="Generate">{icon("generate")}</button></span></label>
  </div>
  <div class="ui-card">
    <h4>Vault items</h4>
    <ul class="items">
      <li><span class="fav g">G</span><span class="it"><b>GitHub</b><small>eslam-dev</small></span><span class="chip">{icon("one-time-code")} 2FA</span></li>
      <li><span class="fav c">{icon("card")}</span><span class="it"><b>Visa ending 4821</b><small>Expires 08/29</small></span></li>
      <li><span class="fav n">{icon("note")}</span><span class="it"><b>Wi-Fi at home</b><small>Secure note</small></span></li>
    </ul>
  </div>
  <div class="ui-card">
    <h4>Messages</h4>
    <p class="msg ok">{icon("done")} Saved and sealed on this device.</p>
    <p class="msg warn">{icon("warning")} This password appeared in a public leak.</p>
    <p class="msg bad">{icon("lookalike")} This site is paypa1.com, not paypal.com.</p>
    <p><span class="beta">Public beta</span></p>
  </div>
</div>
<table class="tok"><caption>Core tokens (full set in <code>tokens/tokens.css</code> and <code>tokens.json</code>)</caption>
<tbody>
<tr><th>Spacing</th><td>4 · 8 · 12 · 16 · 20 · 24 · 32 · 40 · 48 · 64 px</td></tr>
<tr><th>Radius</th><td>6 px small controls · 10 px buttons and fields · 14 px cards · 20 px sheets</td></tr>
<tr><th>Motion</th><td>220 ms with the "fold" curve <code>cubic-bezier(.2,.8,.2,1)</code>; 120 ms for hovers. Respect reduced motion.</td></tr>
<tr><th>Elevation</th><td>Borders first, shadows only for things that float: menus, popovers, the extension popup.</td></tr>
</tbody></table>''')

    apps = section("apps", "09", "In use",
        "Three places people will meet Amanfold first: the browser extension, the mobile app (shown in Arabic) and the website.",
        f'''<div class="mock-row">
  <figure class="mock">
    <div class="popup">
      <div class="pp-h"><span class="pp-logo">{mk()}<b>amanfold</b></span><span class="beta sm">Beta</span><button class="iconbtn" type="button" aria-label="Settings">{icon("settings")}</button></div>
      <label class="pp-search" for="ppS">{icon("search")}<input id="ppS" placeholder="Search your vault"></label>
      <p class="pp-site">{icon("website")} Matches for <b>github.com</b></p>
      <ul class="items pp">
        <li><span class="fav g">G</span><span class="it"><b>GitHub</b><small>eslam-dev</small></span><button class="b primary sm" type="button">Fill</button></li>
        <li><span class="fav g">G</span><span class="it"><b>GitHub (work)</b><small>eslam@company.com</small></span><button class="b secondary sm" type="button">Fill</button></li>
      </ul>
      <div class="pp-foot"><span>{icon("sealed")} Vault sealed · synced 2 min ago</span></div>
    </div>
    <figcaption>Browser extension popup, 360 px wide.</figcaption>
  </figure>
  <figure class="mock">
    <div class="phone" dir="rtl" lang="ar">
      <div class="ph-status"><span>٩:٤١</span><span>●●●</span></div>
      <div class="ph-h"><h5>الخزنة</h5><span class="appicon s28">{mk("rev")}</span></div>
      <label class="pp-search" for="phS">{icon("search")}<input id="phS" placeholder="ابحث في خزنتك"></label>
      <div class="cats"><span class="on">الكل</span><span>الحسابات</span><span>البطاقات</span><span>الملاحظات</span></div>
      <div class="paycard"><span class="pc-fold"></span><span class="pc-brand">VISA</span><span class="pc-num" dir="ltr">•••• •••• •••• 4821</span><span class="pc-name">بطاقة الراتب</span><span class="pc-exp" dir="ltr">08/29</span></div>
      <ul class="items">
        <li><span class="fav g">G</span><span class="it"><b>Gmail</b><small><bdi>eslam@gmail.com</bdi></small></span></li>
        <li><span class="fav b2">ب</span><span class="it"><b>البنك الأهلي</b><small>حساب شخصي</small></span></li>
        <li><span class="fav n">{icon("family")}</span><span class="it"><b>خزنة العائلة</b><small>٤ أعضاء</small></span></li>
      </ul>
      <nav class="ph-tabs"><span class="on">{icon("folder")}الخزنة</span><span>{icon("generate")}المولّد</span><span>{icon("warning")}الأمان</span><span>{icon("settings")}الإعدادات</span></nav>
    </div>
    <figcaption>Mobile app in Arabic, mirrored right to left. Card numbers stay left to right.</figcaption>
  </figure>
</div>
<figure class="mock wide">
  <div class="site">
    <nav class="site-n">{lockup(cls="sm")}<span class="links"><a>Features</a><a>Pricing</a><a>Security</a><a>Open source</a></span><span class="lang">English · العربية</span></nav>
    <div class="site-hero">
      <div class="sh-copy">
        <span class="beta">Public beta · open source</span>
        <h3>Everything you keep, safely folded.</h3>
        <p>Passwords, cards and notes, encrypted on your own devices before they go anywhere. Free on every device you own. Premium is $2.99 a month; Family is $4.99 for up to six people.</p>
        <div class="btns"><button class="b seal" type="button">Get Amanfold free</button><button class="b secondary" type="button">Read how it works</button></div>
      </div>
      <div class="sh-art">{mk("site-mk")}</div>
    </div>
  </div>
  <figcaption>Website hero. The only saffron button on the page is the main call to action.</figcaption>
</figure>''')

    files = section("files", "10", "Files",
        "Everything on this page is generated from code in the repository, so the logo, icons and colours can never drift apart.",
        '''<table class="tok files"><tbody>
<tr><th><code>brand/logo/</code></th><td>Mark and lockups (Latin, Arabic, stacked) in colour, reversed, black and white, as SVG and PNG.</td></tr>
<tr><th><code>brand/app-icon/</code></th><td>Store icon, rounded preview, Android adaptive and themed layers, favicon (SVG and ICO), PNG sizes 16 to 1024.</td></tr>
<tr><th><code>brand/icons/</code></th><td>24 interface icons and a sprite sheet.</td></tr>
<tr><th><code>brand/tokens/</code></th><td>Colours, type, spacing, radius and motion as CSS variables and JSON, for light and dark themes.</td></tr>
<tr><th><code>brand/tools/</code></th><td>The scripts that build all of the above. Change <code>palette.py</code> and re-run to update every file.</td></tr>
</tbody></table>
<p class="note">Before launch: register the name and mark (trademark classes 9 and 42) and buy amanfold.com. Both are still open.</p>''')

    nav = "".join(f'<a href="#{i}">{t}</a>' for i, t in [("idea", "Idea"), ("logo", "Logo"), ("icon", "App icon"), ("colour", "Colour"), ("type", "Type"), ("voice", "Voice"), ("icons", "Icons"), ("ui", "Interface"), ("apps", "In use"), ("files", "Files")])

    tpl = (ROOT / "tools" / "guide.tpl.html").read_text(encoding="utf-8")
    return (tpl.replace("%%NAV%%", nav).replace("%%HERO%%", hero)
            .replace("%%BODY%%", idea + logo + appicon + colour + type_ + voice + iconog + ui + apps + files))


if __name__ == "__main__":
    out = ROOT / "guide" / "index.html"
    out.parent.mkdir(exist_ok=True)
    out.write_text(page(), encoding="utf-8")
    print("wrote", out)
