// Is a field really in front of the person? Values go only into fields that
// are rendered, not transparent, big enough to see, and not covered by
// something else. Hidden decoy fields (a common way to harvest autofill) and
// fields under an overlay (clickjacking) are skipped.

type Control = HTMLInputElement | HTMLSelectElement;

function styleHidden(el: Element): boolean {
  let opacity = 1;
  for (let cur: Element | null = el; cur; cur = cur.parentElement ?? (cur.getRootNode() as ShadowRoot).host ?? null) {
    const cs = getComputedStyle(cur);
    if (cs.display === "none" || cs.visibility === "hidden" || cs.visibility === "collapse") return true;
    if (cs.contentVisibility === "hidden") return true;
    opacity *= parseFloat(cs.opacity || "1");
    if (opacity < 0.2) return true;
    if (cur instanceof HTMLElement && cur.hidden) return true;
    if (cur === cur.ownerDocument.documentElement) break;
  }
  return false;
}

function covered(el: Element, r: DOMRect): boolean {
  // Sample a few points inside the field; each must hit the field itself
  // (or something inside it, such as a shadow part).
  const pts: [number, number][] = [
    [r.left + r.width / 2, r.top + r.height / 2],
    [r.left + Math.min(8, r.width / 4), r.top + r.height / 2],
    [r.right - Math.min(8, r.width / 4), r.top + r.height / 2],
  ];
  for (const [x, y] of pts) {
    const root = el.getRootNode() as Document | ShadowRoot;
    const hit = root.elementFromPoint?.(x, y) ?? document.elementFromPoint(x, y);
    if (!hit) return true;
    if (hit !== el && !el.contains(hit) && !(hit instanceof HTMLLabelElement && hit.control === el)) return true;
  }
  return false;
}

export function fillable(el: Control): boolean {
  if (!el.isConnected || el.disabled) return false;
  if (el instanceof HTMLInputElement && (el.readOnly || el.type === "hidden")) return false;
  if (el.checkVisibility && !el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true } as CheckVisibilityOptions)) {
    return false;
  }
  if (styleHidden(el)) return false;
  let r = el.getBoundingClientRect();
  if (r.width < 4 || r.height < 4) return false;
  const vw = document.documentElement.clientWidth || innerWidth;
  const vh = document.documentElement.clientHeight || innerHeight;
  const inView = () => r.bottom > 0 && r.right > 0 && r.top < vh && r.left < vw;
  if (!inView()) {
    // Off-screen fields (left: -9999px) stay unfilled; a form below the
    // fold is scrolled to first so it can be checked like the rest.
    const docX = r.left + scrollX;
    const docY = r.top + scrollY;
    if (docX + r.width <= 0 || docY + r.height <= 0) return false;
    el.scrollIntoView({ block: "center", inline: "nearest" });
    r = el.getBoundingClientRect();
    if (!inView()) return false;
  }
  return !covered(el, r);
}

/**
 * Where the browser supports it (Chrome), also ask whether the field is
 * visible to the person including every frame around it: catches a frame
 * made transparent or covered by the page that embeds it.
 */
export function visibleInFrames(el: Element): Promise<boolean> {
  const Entry = (globalThis as { IntersectionObserverEntry?: { prototype: object } }).IntersectionObserverEntry;
  if (!Entry || !("isVisible" in Entry.prototype)) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      io.disconnect();
      resolve(false);
    }, 1000);
    const io = new IntersectionObserver(
      (entries) => {
        const e = entries[entries.length - 1] as IntersectionObserverEntry & { isVisible?: boolean };
        clearTimeout(timer);
        io.disconnect();
        resolve(e.isVisible === true);
      },
      { trackVisibility: true, delay: 100 } as IntersectionObserverInit,
    );
    io.observe(el);
  });
}
