# Touch and pen

Status: shared native/WebAssembly input implementation, with synthetic GUI regressions.
Physical tablet, stylus and mobile-device qualification remains separate from these tests.

Preferences → Touch and pen offers **Auto**, **On** and **Off**. Auto enables the larger
drawing toolbar after the first completed touch interaction. It does not rearrange the page
during that interaction. On also suits pen tablets whose drivers provide mouse events.
The choice is saved; an unknown setting falls back to Auto.

The toolbar provides 44-point targets for Hand, Select, Draw, Highlight, sticky notes, text
boxes, Eraser, Undo and Redo. It scrolls horizontally when the window is narrow. Drawing
colour, opacity and width stay available beside the tools. Existing PDF annotation permissions,
editable annotation storage and undo/redo apply normally.

With a drawing or highlighting tool selected, a single contact edits the page. Hand moves
the page with one contact. Two contacts move and pinch-zoom the page; adding the second
contact cancels an unfinished mark. Releasing the final finger cannot create a stray dot.
Touch cancellation or focus loss cancels the unfinished gesture. An ordinary pen tap with
Draw creates a dot. All pointer samples delivered within a frame are retained, including
the release position when the touch backend immediately removes the pointer. Long strokes
are bounded to 65,536 samples and progressively thinned instead of growing without limit.

The same code is compiled for Windows, macOS, Linux/FreeBSD and WebAssembly, including mobile
WebViews. The operating system/browser supplies the input. This change does not advertise
pressure/tilt-sensitive ink, hardware eraser buttons or pen-specific palm rejection: the
current shared input backend does not reliably identify every pen separately from fingers.
Two-finger cancellation is a gesture safeguard, not a pen classifier.

Automation uses the existing engine `Edit::AddAnnotation` and `comment_add` tool for document
edits. The opt-in UI control channel accepts `ui.set` with `touch-mode=auto|on|off`; `ui.state`
reports `touch_mode` and `touch_active`. Tests exercise real shell input, saved PDF InkList
coordinates, highlighting, cancellation, navigation, undo/redo and control-channel selection.
