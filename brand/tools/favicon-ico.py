"""Pack favicon PNGs into favicon.ico. Run after render-png.js."""
from pathlib import Path
from PIL import Image
d = Path(__file__).resolve().parent.parent / "app-icon"
img = Image.open(d / "png/favicon-48.png")
img.save(d / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)])
print("wrote", d / "favicon.ico")
