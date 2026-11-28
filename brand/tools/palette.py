"""Amanfold colour palette. Single source of truth for tokens, logos and icons."""
GREEN = {50: "#EAF7F1", 100: "#CFEFE0", 200: "#9BE4C4", 300: "#5FCFA3", 400: "#2CB183",
         500: "#13906A", 600: "#0B6E4F", 700: "#09593F", 800: "#084632", 900: "#0A3326"}
AMBER = {50: "#FEF7E8", 100: "#FCEBC5", 200: "#FADB96", 300: "#F7C768", 400: "#F5B544",
         500: "#E39A1F", 600: "#B87610", 700: "#8F590D", 800: "#6B430E", 900: "#4A2F0D"}
NEUTRAL = {0: "#FFFFFF", 50: "#F4F6F3", 100: "#EAEEEA", 200: "#DDE3DE", 300: "#C2CCC6", 400: "#93A29B",
           500: "#6E7F78", 600: "#55655F", 700: "#3A4A44", 800: "#1F302A", 900: "#0C1F1A", 950: "#081410"}
SEMANTIC = {
    "success": {"light": "#0B6E4F", "dark": "#5FCFA3"},
    "warning": {"light": "#8F590D", "dark": "#F7C768"},
    "danger":  {"light": "#B3261E", "dark": "#F2877B"},
    "info":    {"light": "#1F5FA8", "dark": "#8DB8EA"},
}
# Named brand colours
AMAN_GREEN = GREEN[600]   # logo, buttons, links
MINT = GREEN[200]         # the fold; links on dark
INK = NEUTRAL[900]        # text, dark surfaces
PAPER = NEUTRAL[50]       # backgrounds
HIGHLIGHT = AMBER[400]    # one highlight per screen
ICON_GRADIENT = ("#128760", "#085A40")   # app icon tile, top left to bottom right

# Logo colours per background: body of the letter, the folded corner, the name
LOGO_ON_LIGHT = dict(body=AMAN_GREEN, flap=MINT, text=INK)
LOGO_ON_DARK = dict(body="#FFFFFF", flap=MINT, text="#FFFFFF")


def luminance(hex_):
    c = [int(hex_[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    c = [v / 12.92 if v <= .03928 else ((v + .055) / 1.055) ** 2.4 for v in c]
    return .2126 * c[0] + .7152 * c[1] + .0722 * c[2]


def contrast(a, b):
    la, lb = sorted([luminance(a), luminance(b)], reverse=True)
    return (la + .05) / (lb + .05)
