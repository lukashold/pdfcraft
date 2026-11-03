# Touch and pen

Status: shared native/WebAssembly input implementation, with synthetic GUI regressions.
Physical tablet, stylus and mobile-device qualification remains separate from these tests.

Preferences ▸ Appearance ▸ Touch and pen offers **Auto**, **On** and **Off**. Auto enables the larger
drawing toolbar after the first completed touch interaction. It does not rearrange the page
during that interaction. On also suits pen tablets whose drivers provide mouse events.
The choice is saved; an unknown setting falls back to Auto.

The toolbar provides 44-point targets for Hand, Select, Draw, Highlight, sticky notes, text
boxes, Eraser, Undo and Redo, in place of the quick bar. Read mode and full screen hide it, as
they hide the quick bar. It scrolls horizontally when the window is narrow. Drawing
colour, opacity and width stay available beside the tools. Existing PDF annotation permissions,
editable annotation storage and undo/redo apply normally.

With a drawing or highlighting tool selected, a single contact edits the page. Hand moves
the page with one contact. Two contacts move and pinch-zoom the page; adding the second
contact cancels an unfinished mark. Releasing the final finger cannot create a stray dot.
Touch cancellation or focus loss cancels the unfinished gesture. Cancelling works as Escape
does on a press the pages hold (`DocView::cancel_press`): nothing is drawn and the press is
ignored until the button is up. An ordinary pen tap with Draw creates a dot. Every pointer
sample delivered within a frame is kept: page modules read them from `PagePointer::samples`
(up to 1,024 a frame), and a stroke ends at the release position, also when the touch backend
immediately removes the pointer. Long strokes are bounded to 65,536 points and progressively
thinned instead of growing without limit.

The same code is compiled for Windows, macOS, Linux/FreeBSD and WebAssembly, including mobile
WebViews. The operating system/browser supplies the input. This change does not advertise
pressure/tilt-sensitive ink, hardware eraser buttons or pen-specific palm rejection: the
current shared input backend does not reliably identify every pen separately from fingers.
Two-finger cancellation is a gesture safeguard, not a pen classifier.

## Stylus mode (browser)

In the browser eframe reports pens and fingers as one pointer, but the browser itself names
each one (`pointerType`). `stylus::install` listens for that ahead of eframe, in the capture
phase on the window. Stylus mode turns on the first time a pen is used and is remembered for
later visits; `?stylus=off` keeps it off and `?stylus=on` starts with it on. Native shells are
unchanged.

In stylus mode the pen keeps the chosen tool, and a finger moves the page with every tool the
pen owns (Select, markup, Draw, redaction, measuring, marquee zoom, snapshots). Tap-to-place
tools keep the finger. The app reads what made the latest press once per frame
(`PdfCraftApp::stylus`); the pages see it as `PagePointer::kind`. A finger press with a pen tool
pans by the Hand tool's path and the pages give it up with `DocView::cancel_press`, so it marks
nothing. The pen's own touch events (Chrome on Android sends them) don't count as a finger.

Touches that start while the pen is down, or within 0.3 s of its lift, are dropped as palms
before eframe sees them. The timings come from a Pixel Tablet in Chrome, where Android already
ignores a resting palm and deliberate finger swipes came 0.5-0.8 s after the pen. iPad and
Windows pens in the browser are untested, and native pens are a separate matter.

Automation uses the existing engine `Edit::AddAnnotation` and `comment_add` tool for document
edits. The opt-in UI control channel accepts `ui.set` with `touch-mode=auto|on|off`; `ui.state`
reports `touch_mode` and `touch_active`. Tests exercise real shell input, saved PDF InkList
coordinates, highlighting, cancellation, navigation, undo/redo and control-channel selection.

## Revision history

- 2026-10-11: first version (#1108).
- 2026-11-02: reconciled with the page-input rules: samples and cancellation go through
  `PagePointer` and the pages' press cancellation; the toolbar follows Read mode and full screen.
- 2026-11-02: stylus mode in the browser: the pen draws, a finger moves the page, palms are
  dropped.
