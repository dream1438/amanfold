"""Amanfold colour palette. Single source of truth for tokens, logos and icons."""
OASIS = {50: "#EEF6F4", 100: "#D5E9E4", 200: "#B0D3CA", 300: "#8DBAB0", 400: "#5E9A8E",
         500: "#3B7D71", 600: "#26665B", 700: "#18514A", 800: "#0F3B3A", 900: "#0A2B2A", 950: "#061B1A"}
SAFFRON = {50: "#FDF6E9", 100: "#FBEACB", 200: "#F6D395", 300: "#F0BB62", 400: "#EDAE4C",
           500: "#E9A23B", 600: "#CC8420", 700: "#A2661A", 800: "#7B4D17", 900: "#573813"}
MIST = {0: "#FFFFFF", 50: "#F6F8F7", 100: "#EDF2F0", 200: "#DCE4E1", 300: "#C3CFCB", 400: "#96A6A1",
        500: "#6A7D78", 600: "#4F625E", 700: "#374845", 800: "#1F2F2D", 900: "#13211F", 950: "#0B1716"}
SEMANTIC = {
    "success": {"light": "#1E7A50", "dark": "#5CC495"},
    "warning": {"light": "#A35F00", "dark": "#F0BB62"},
    "danger":  {"light": "#B93A27", "dark": "#F08A78"},
    "info":    {"light": "#2D64A3", "dark": "#8DB8EA"},
}
# Mark colours per background
MARK_ON_LIGHT = dict(L=OASIS[300], D=OASIS[800], S=SAFFRON[500])
MARK_ON_DARK = dict(L=OASIS[50], D=OASIS[400], S=SAFFRON[500])
