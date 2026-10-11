# PdfCraft Hot Folder

**Status: implementation in progress — not a completed ABBYY replacement.**

The optional standalone application processes folders without keeping the PdfCraft editor open.
It shares OCR, orientation correction, deskew and paper-edge detection with the engine. Its
interface follows the primary system language when English, German, French, Dutch, Spanish,
Norwegian Bokmål or Swedish is available, otherwise English. The document language defaults
independently from the system language, using the shared OCR language list.

## Current processing contract

- Create named tasks with separate existing input/output folders. Local folders and network
  folders mounted as filesystem paths use the same scanner; credentials are managed by the OS.
- Choose PDFs, PNG/JPEG/TIFF/BMP images, optional subfolders, start time, once or recurring checks,
  and how long a file must remain unchanged before processing. No recursive symlink traversal.
  Daily, weekly and monthly schedules use local wall-clock time. A missing month day uses that
  month's last day; a DST gap uses the next valid local minute, and repeated times run once.
- Configure resolution, language, existing-text skipping, recognition, rotation, straightening,
  paper-edge cropping and safety padding. Optional blank-page removal is deliberately conservative:
  only completely white rendered pages without extracted text are removed; documents with forms,
  links or annotations are kept. An entirely blank input produces an error rather than an empty PDF.
- Configure up to sixteen saving steps: processed or image-only PDF, DOCX, RTF, HTML, text,
  PNG, JPEG or TIFF. Group per file, per containing folder or across a complete settled run.
  File/date/time/task/folder/counter templates control names. Each conversion is staged before
  publishing. Existing names receive numbered suffixes; byte-identical retries reuse results.
  Original move/delete follows success of every saving step and its durable receipt. A failed
  grouped batch requires an explicit retry, so missing inputs cannot silently disappear from it.
- Select one-based page ranges and save recognition-area templates in task JSON. Regions use
  displayed top-left percentages, include or exclude OCR words, and leave visible content intact.
  Optional grayscale, inversion, brightness, contrast, black/white threshold, median noise reduction
  and splitting spreads rasterize selected pages at the requested resolution (bounded by renderer
  limits). Interactive forms/links/annotations are refused for rasterizing image changes.
- Optional blank-page archives preserve removed pages as separate PDFs; originals are not removed
  until these files also save successfully. An entirely blank input requires an archive destination.
- Durable receipts suppress duplicate processing across worker restarts. Failed files are logged
  once, retried when their content changes or through Retry/Start. Stop is checked between pages
  and does not save partial OCR output.
- Import/export task JSON, duplicate tasks, inspect the last 1,000 log events, filter failures and
  open output folders. Importing never enables tasks automatically.

The scanner caps input batches at 512 MiB/128 files, enumeration and journal entries at 10,000, nesting at 32,
configuration/state JSON at 16 MiB and tasks at 32. These bounds produce actionable errors.
Files must be closed by their producer before processing; for slow/scattered writes increase the
settling time or have the producer rename the completed file into the input folder.

## Agent and service interfaces

`pdfcraft-hot-folder --config tasks.json` opens the separate configuration application.
`pdfcraft-hot-folder --worker --config tasks.json` runs the worker without a window.
`pdfcraft-hot-folder --once --config tasks.json` executes one scheduler pass and prints JSON.
`pdfcraft-hot-folder --service --config tasks.json` hosts the installed system service.
`--service-action status|start|stop|enable|disable|install|uninstall` explicitly administers it.
Unix installation additionally requires `--account USER`, a non-root service account.
`--initialize-config --config PATH` atomically creates a stopped configuration without replacing
an existing file. Unix new directories/files are owner-only. Run it as the service account.
The GUI uses an installed matching service rather than launching a competing worker. Windows
installation/removal uses the optional MSI feature; runtime APIs never install it implicitly.
The worker holds an OS lock so only one process owns a configuration, and each engine pass takes
a second lock shared with agent calls. It does not open a network listener or an MCP server.

`hot_folder_configure`, `hot_folder_status`, `hot_folder_poll` and `hot_folder_retry` expose the
same engine through `pdfcraft-cli run` and opt-in MCP. All nested paths honour `--root`, including
inputs, every saving/blank archive folder, configuration and sidecars. Poll processes one stable
file or complete bounded batch per task per due interval; it never silently starts a background worker.

Example task (folders must already exist):

```json
{
  "tasks": [{
    "id": "school-scans",
    "name": "School scans",
    "input": "/scans/in",
    "output": "/scans/out",
    "enabled": false,
    "recursive": true,
    "interval_seconds": 10,
    "settle_seconds": 3,
    "originals": "keep",
    "settings": { "language": "de", "auto_crop": true }
  }]
}
```

`tasks.state.json` stores receipts, queue observations and logs. `tasks.lock` and
`tasks.worker.lock` are OS locks, released automatically after a crash. Corrupt JSON is reported
without resetting or overwriting it. Do not delete the journal unless you intentionally want
to process inputs again. Keep it together with its configuration when archiving a completed job.

## Functional reference and remaining scope

The clean-room reference is ABBYY's [Hot Folder documentation](https://help.abbyy.com/en-us/finereader/16/user_guide/hotfolder/).
No ABBYY assets or code are included. A processing scheduler and task settings are the relevant
functional behaviors; this document does not claim compatibility with ABBYY project/task files.

The expanded request also requires the following work before a complete release:

| Requirement | Current state |
|---|---|
| Windows/macOS/Linux service at system boot | Service host and explicit administration integrated; platform installers and cross-platform verification in progress |
| Once / recurring interval and local start time, daily/weekly/monthly | Implemented; each due run drains its stable input queue before scheduling the next calendar occurrence |
| Local and OS-mounted network folders | Implemented |
| FTP and Outlook sources | Dedicated authenticated source adapters and credentials handling remain |
| ABBYY project source | Format/interoperability investigation required; not falsely offered as supported |
| OCR / scan corrections / exact searchable PDF | Implemented through existing engine |
| Saved region templates / preprocessing | Page selection, include/exclude regions, grayscale/inversion/threshold/brightness/contrast/median filter and spread splitting implemented; analysis-only reporting remains |
| Blank-page removal and separate blank-page archive | Implemented with conservative white-only detection and durable archive receipts |
| Multiple save steps/formats | Nine real exporter formats and multiple saving steps implemented; XLSX/EPUB exporters are not currently available in PdfCraft |
| Group output by file, folder or entire task | Implemented with a 128-file/512-MiB batch bound and staged publication |
| Import/export, duplicate, stop, retry, logs and failure filtering | Implemented for native task JSON |
| Seven requested UI languages | Complete label catalogs and system-language fallback implemented |

Do not run a privileged root/LocalSystem worker against configuration editable by unprivileged
users. System-service installation must establish an appropriately restricted account and
configuration ownership. Installing the optional component must not process any folder until
a user explicitly enables a task.
