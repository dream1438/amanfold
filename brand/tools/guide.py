"""Generate the Amanfold brand guide (brand/guide/index.html) from the same sources as the assets.
Run from the repository root after build.py and icons.py."""
from pathlib import Path

import build
from mark import symbol_group, f as fmt
from palette import GREEN, AMBER, NEUTRAL, SEMANTIC, AMAN_GREEN, INK, MINT, PAPER, HIGHLIGHT, contrast
from icons import ICONS, el

ROOT = Path(__file__).resolve().parent.parent
X = dict(body="x", flap="x", text="x")      # placeholder colours: the guide paints through CSS classes


def mk(cls="", label=None, kind="full"):
    """Inline symbol whose colours come from CSS custom properties (theme-aware)."""
    a = f'role="img" aria-label="{label}"' if label else 'aria-hidden="true"'
    return f'<svg class="mk {cls}" viewBox="4 6 54 52" {a}>{symbol_group("x", "x" if kind == "full" else None, kind, classes=True)}</svg>'


def inline(dims, cls, label=None):
    w, h, body = dims
    a = f'role="img" aria-label="{label}"' if label else 'aria-hidden="true"'
    return f'<svg class="{cls}" viewBox="0 0 {fmt(w)} {fmt(h)}" style="aspect-ratio:{fmt(w)}/{fmt(h)}" {a}>{body}</svg>'


def icon(name, cls="ic"):
    return f'<svg class="{cls}" viewBox="0 0 24 24" aria-hidden="true">{"".join(el(p) for p in ICONS[name])}</svg>'


def lockup(rtl=False, cls="", one=False):
    colors = dict(X, flap=None) if one else X
    return inline(build.lockup(colors, arabic=rtl, classes=True), f'lockup {"ar " if rtl else ""}{cls}', "أمانفولد" if rtl else "Amanfold")


def wordmark(cls="wm"):
    return inline(build.wordmark_only(False, "x", classes=True), cls)


def sized(px, kind="full"):
    return f'<span style="width:{px}px;height:{px}px;display:block">{mk(kind=kind)}</span>'


def cr(a, b):
    return f"{contrast(a, b):.1f} : 1"


def swatches(name, scale, brand_step, note):
    cells = []
    for k, v in scale.items():
        dark_text = NEUTRAL[0] if (int(v[1:3], 16) * .299 + int(v[3:5], 16) * .587 + int(v[5:7], 16) * .114) < 150 else NEUTRAL[950]
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
    <div class="hero-mark" id="heroMark">{mk("big", "Amanfold symbol")}</div>
    <div class="hero-copy">
      <p class="kicker">Brand guide · version 2.0 · November 2026</p>
      <h1>{wordmark("wm hero-wm")}<span class="sr">Amanfold</span></h1>
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
    <p>The symbol is the first letter of the name, a lowercase <strong>a</strong>, and it carries both words. The opening of the letter is a <strong>keyhole</strong>: that is <em>aman</em>, the promise that what you keep here is locked and only you hold the key. The top corner of the letter is <strong>folded over</strong> like a private note: that is <em>fold</em>.</p>
    <p>It can be described in one sentence, "an a with a keyhole and a folded corner", and it is drawn as one flat shape in one colour family so that it stays clear on a phone, in a browser toolbar and in print. Because it is built from the name itself, it can only be Amanfold.</p>
    <p>The same two ideas run through the brand. Green is the colour every app already uses for "safe", so the brand colour and the good state agree. Cards, notes and other containers in the interface carry one folded corner.</p>
  </div>
  <figure class="anatomy onlight">
    {mk("anat")}
    <figcaption>
      <ol class="anat-list">
        <li><b>The letter a.</b> A round bowl and a straight stem, the same construction as the letters in the name.</li>
        <li><b>The keyhole.</b> Centred in the bowl. Its round head is one third of the bowl's width.</li>
        <li><b>The fold.</b> Exactly as wide as the stem, in Mint. In one colour it becomes a cut line.</li>
      </ol>
    </figcaption>
  </figure>
</div>''')

    logo = section("logo", "02", "Logo",
        "Use the full logo wherever there is room, and the symbol alone where the name is already present, such as an app icon or a browser toolbar.",
        f'''<div class="logo-grid">
  <figure class="tile light">{lockup()}<figcaption>Main logo · <code>amanfold-logo.svg</code></figcaption></figure>
  <figure class="tile light">{lockup(rtl=True)}<figcaption>Arabic logo, symbol on the right, never mirrored · <code>amanfold-logo-arabic.svg</code></figcaption></figure>
  <figure class="tile dark">{lockup(cls="rev")}<figcaption>Reversed, on Ink · <code>amanfold-logo-reversed.svg</code></figcaption></figure>
  <figure class="tile light">{inline(build.stacked(X, classes=True), "stacked", "Amanfold")}<figcaption>Stacked, for splash screens and stickers · <code>amanfold-logo-stacked.svg</code></figcaption></figure>
</div>
<h3 class="sub">Three drawings for three jobs</h3>
<div class="three-d">
  <figure class="draw"><div class="row">{sized(64)}{sized(32)}</div><figcaption><b>Full colour</b>Aman Green with a Mint fold. The main symbol, for 32 px and above. <code>amanfold-mark.svg</code></figcaption></figure>
  <figure class="draw oneink"><div class="row">{sized(64, "crease")}{sized(32, "crease")}</div><figcaption><b>One colour</b>The fold is a cut line, so it survives in black, white, stamps and engraving. <code>amanfold-mark-black.svg</code></figcaption></figure>
  <figure class="draw"><div class="row">{sized(24, "small")}{sized(16, "small")}</div><figcaption><b>Small</b>For 24 px and below: a plain cut corner and a larger keyhole that stays open at 16 px. <code>amanfold-mark-small.svg</code></figcaption></figure>
</div>
<div class="rules">
  <figure class="rule">
    <div class="clear">{lockup()}</div>
    <figcaption><b>Clear space</b> equals the width of the symbol's stem (a quarter of its height) on every side. Keep text, edges and other logos outside it.</figcaption>
  </figure>
  <figure class="rule">
    <div class="minsz"><span>{sized(16, "small")}<small>16 px</small></span><span>{lockup()}<small>96 px wide</small></span></div>
    <figcaption><b>Minimum size.</b> The small symbol holds up at 16 px. Use the full logo only from 96 px wide; below that, use the symbol alone.</figcaption>
  </figure>
</div>
<p class="note"><b>How the name is drawn.</b> The name is set in Readex Pro at weight 600 and then redrawn: both letters a are replaced by a round one-storey a that matches the symbol, spacing is 1.2% tighter, and the pairs n f, f o and l d are adjusted by hand. The symbol is as tall as the letters f, l and d, and the gap between symbol and name is one stem width. Always use the logo files, never type the name in a font.</p>
<h3 class="sub">Please don't</h3>
<ul class="donts onlight">
  <li><div class="dz">{mk("x-flip")}</div><span>Mirror it, including in Arabic and other right-to-left layouts.</span></li>
  <li><div class="dz">{mk("x-col")}</div><span>Recolour the letter or the fold outside the palette.</span></li>
  <li><div class="dz">{mk("x-str")}</div><span>Stretch, squash, rotate or outline it.</span></li>
  <li><div class="dz x-lock">{mk()}<svg viewBox="0 0 24 24" class="pad" aria-hidden="true"><path d="M7 11V8a5 5 0 0 1 10 0v3"/><rect x="5" y="11" width="14" height="10" rx="2"/></svg></div><span>Add padlocks, shields or keys. The keyhole already says it.</span></li>
</ul>''')

    appicon = section("icon", "03", "App icon",
        "One icon for every platform: the white symbol with its Mint fold on an Aman Green tile with a soft light from the top left. Stores and launchers apply their own corner masks, so the source is a full-bleed square.",
        f'''<div class="icon-row">
  <div class="phone-row">
    <div class="homescreen">
      <span class="app"><i class="ph p1"></i><small>Photos</small></span>
      <span class="app"><i class="ph p2"></i><small>Mail</small></span>
      <span class="app"><span class="appicon">{mk()}</span><small>Amanfold</small></span>
      <span class="app"><i class="ph p3"></i><small>Maps</small></span>
    </div>
    <p class="cap">On a home screen beside other apps. The symbol sits slightly right of centre inside the tile, because its heavy stem makes a mathematically centred symbol look off-centre.</p>
  </div>
  <div class="sizes">
    <span class="appicon s120">{mk()}</span><span class="appicon s64">{mk()}</span><span class="appicon s40">{mk()}</span>
    <span class="appicon s24">{mk(kind="small")}</span><span class="appicon s16">{mk(kind="small")}</span>
  </div>
  <div class="tabbar"><span class="tab onlight">{'<span class="s16" style="display:block;height:16px">' + mk(kind="small") + '</span>'}Amanfold · Your vault</span><span class="tab dim">Inbox</span><span class="tab dim">Docs</span></div>
</div>
<p class="note">Tiles of 48 px and below use the small drawing. The browser tab icon (<code>favicon.svg</code>) is the bare symbol with no tile; it is Aman Green on light browser themes and white on dark ones. Files: <code>app-icon/app-icon-1024.svg</code> (iOS, macOS, Windows, stores), <code>android-adaptive-foreground.svg</code> + <code>android-adaptive-background.svg</code> + <code>android-monochrome.svg</code> (Android 13 themed icons), <code>favicon.svg</code> and <code>favicon.ico</code>, plus PNGs from 16 to 1024 px in <code>app-icon/png/</code>.</p>''')

    colour = section("colour", "04", "Colour",
        "Aman Green carries the brand. It is the green people already read as safe, and it is not the blue of most security apps. Mint is the fold. Amber is a single warm highlight. Neutrals are tinted towards green so greys never look accidental.",
        f'''{swatches("Green", GREEN, 600, "Primary. Green 600 (outlined) is Aman Green: the logo, primary buttons and links on light. Green 200 is Mint. Green 400 is the button colour on dark screens.")}
{swatches("Amber", AMBER, 400, "The highlight. Amber 400 (outlined) is used for badges and one call to action. Never use it for text on light backgrounds; use Amber 700 instead.")}
{swatches("Neutral", NEUTRAL, None, "Text, surfaces and borders, with a slight green bias. Neutral 900 is Ink and Neutral 50 is Paper.")}
<div class="sem">
  <h4>Status colours</h4>
  <ul>{"".join(f'<li><span class="dot" style="--c:{v["light"]};--cd:{v["dark"]}"></span><b>{k.title()}</b><code>{v["light"]} / {v["dark"]}</code></li>' for k, v in SEMANTIC.items())}</ul>
  <p>Light / dark theme values. Success is the brand green on purpose. Danger red is kept for weak or leaked passwords and destructive actions only, so it never competes with the brand. Status colours always come with an icon and words, never colour alone.</p>
</div>
<div class="seal-rule">
  <div class="srv"><span class="pill-s">Public beta</span></div>
  <div><h4>One highlight per screen</h4><p>Amber appears at most once per screen, on the single thing that most deserves attention: the beta badge, the folded corner of a payment card, the main button on a marketing page. If two things on a screen are amber, one of them is wrong.</p></div>
</div>
<table class="contrast"><caption>Text contrast, measured against WCAG 2.2 (4.5 : 1 passes for normal text)</caption>
<thead><tr><th>Pairing</th><th>Light theme</th><th>Dark theme</th></tr></thead>
<tbody>
<tr><td>Body text on background</td><td>{cr(INK, PAPER)}</td><td>{cr("#EAF2EE", NEUTRAL[950])}</td></tr>
<tr><td>Muted text on surface</td><td>{cr(NEUTRAL[600], "#FFFFFF")}</td><td>{cr("#A3B5AE", "#0F231D")}</td></tr>
<tr><td>Text on primary button</td><td>{cr("#FFFFFF", AMAN_GREEN)}</td><td>{cr(INK, GREEN[400])}</td></tr>
<tr><td>Link on background</td><td>{cr(AMAN_GREEN, PAPER)}</td><td>{cr(MINT, NEUTRAL[950])}</td></tr>
<tr><td>Ink text on Amber</td><td>{cr(INK, HIGHLIGHT)}</td><td>{cr(INK, HIGHLIGHT)}</td></tr>
<tr><td>Danger text on surface</td><td>{cr(SEMANTIC["danger"]["light"], "#FFFFFF")}</td><td>{cr(SEMANTIC["danger"]["dark"], "#0F231D")}</td></tr>
</tbody></table>
<p class="note">Aman Green on Ink is only {cr(AMAN_GREEN, INK)}, which fails. That is why the logo turns white on dark backgrounds and dark screens use Green 400 for buttons.</p>''')

    type_ = section("type", "05", "Typography",
        "Three open-source families (SIL Open Font License) that cover Latin and Arabic with matching rhythm, so English and Arabic screens feel like the same product.",
        '''<div class="faces">
  <article class="face">
    <p class="role">Display · headlines, the wordmark</p>
    <p class="spec d">Readex Pro</p>
    <p class="pair d">Safely folded. <span lang="ar" dir="rtl">مطويٌّ بأمان.</span></p>
    <p class="meta">Weights 500 and 600. The logo's name starts from weight 600. One family designed for both scripts, so mixed headlines keep one rhythm.</p>
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
    <p class="meta">Slashed zero and distinct l, 1 and I. Digits show in Amber 700, symbols in Green 500, so a password can be read aloud without mistakes. Passwords are always left-to-right, even in Arabic.</p>
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
<p class="note">Icons use the text colour by default and Aman Green when active. Arrows and directional icons mirror in right-to-left layouts; everything else, including the sync arrows and the clock, stays as drawn. Files: <code>icons/*.svg</code> and <code>icons/sprite.svg</code>.</p>''')

    ui = section("ui", "08", "Interface basics",
        "Quiet surfaces, generous spacing and one clear action per view. Corners are soft but not round; spacing follows a 4 px grid.",
        f'''<div class="ui-grid">
  <div class="ui-card">
    <h4>Buttons</h4>
    <div class="btns"><button class="b primary" type="button">Save login</button><button class="b secondary" type="button">Cancel</button><button class="b quiet" type="button">{icon("copy")} Copy</button></div>
    <div class="btns"><button class="b seal" type="button">Get Amanfold free</button><span class="cap">Amber button: marketing pages only, once per page.</span></div>
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
    <p class="msg ok">{icon("done")} Saved and encrypted on this device.</p>
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
      <div class="pp-h"><span class="pp-logo">{lockup()}</span><span class="beta sm">Beta</span><button class="iconbtn" type="button" aria-label="Settings">{icon("settings")}</button></div>
      <label class="pp-search" for="ppS">{icon("search")}<input id="ppS" placeholder="Search your vault"></label>
      <p class="pp-site">{icon("website")} Matches for <b>github.com</b></p>
      <ul class="items pp">
        <li><span class="fav g">G</span><span class="it"><b>GitHub</b><small>eslam-dev</small></span><button class="b primary sm" type="button">Fill</button></li>
        <li><span class="fav g">G</span><span class="it"><b>GitHub (work)</b><small>eslam@company.com</small></span><button class="b secondary sm" type="button">Fill</button></li>
      </ul>
      <div class="pp-foot"><span>{icon("locked")} Vault locked · synced 2 min ago</span></div>
    </div>
    <figcaption>Browser extension popup, 360 px wide.</figcaption>
  </figure>
  <figure class="mock">
    <div class="phone" dir="rtl" lang="ar">
      <div class="ph-status"><span>٩:٤١</span><span>●●●</span></div>
      <div class="ph-h"><h5>الخزنة</h5><span class="appicon s28">{mk(kind="small")}</span></div>
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
      <div class="sh-art onlight">{mk("site-mk")}</div>
    </div>
  </div>
  <figcaption>Website hero. The only amber button on the page is the main call to action.</figcaption>
</figure>''')

    files = section("files", "10", "Files",
        "Everything on this page is generated from code in the repository, so the logo, icons and colours can never drift apart.",
        '''<table class="tok files"><tbody>
<tr><th><code>brand/logo/</code></th><td>Symbol (full colour, one colour, small) and logos (Latin, Arabic, stacked) in colour, reversed, black and white, as SVG and PNG.</td></tr>
<tr><th><code>brand/app-icon/</code></th><td>Store icon, rounded preview, Android adaptive and themed layers, favicon (SVG and ICO), PNG sizes 16 to 1024.</td></tr>
<tr><th><code>brand/icons/</code></th><td>24 interface icons and a sprite sheet.</td></tr>
<tr><th><code>brand/tokens/</code></th><td>Colours, type, spacing, radius and motion as CSS variables and JSON, for light and dark themes.</td></tr>
<tr><th><code>brand/tools/</code></th><td>The scripts that build all of the above. Colours live in <code>palette.py</code> and the symbol's geometry in <code>mark.py</code>; change them and re-run to update every file.</td></tr>
</tbody></table>
<p class="note">Before launch: run a trademark similarity search and register the name and symbol (classes 9 and 42), because a keyhole inside a letter is not a new idea; buy amanfold.com; have a native Arabic type designer check the Arabic name; and show the logo to a few real users.</p>''')

    nav = "".join(f'<a href="#{i}">{t}</a>' for i, t in [("idea", "Idea"), ("logo", "Logo"), ("icon", "App icon"), ("colour", "Colour"), ("type", "Type"), ("voice", "Voice"), ("icons", "Icons"), ("ui", "Interface"), ("apps", "In use"), ("files", "Files")])

    tpl = (ROOT / "tools" / "guide.tpl.html").read_text(encoding="utf-8")
    return (tpl.replace("%%NAV%%", nav).replace("%%HERO%%", hero)
            .replace("%%BODY%%", idea + logo + appicon + colour + type_ + voice + iconog + ui + apps + files))


if __name__ == "__main__":
    out = ROOT / "guide" / "index.html"
    out.parent.mkdir(exist_ok=True)
    out.write_text(page(), encoding="utf-8")
    print("wrote", out)
