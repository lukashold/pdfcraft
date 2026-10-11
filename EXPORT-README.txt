Historical OCR model conversion (ocr-historical-v1)
================================================

This is a model-maintenance package. PdfCraft itself is Rust-only and does not
install or execute Python. Users download only the ONNX model and dictionary.

Original weights: Benjamin Kiessling / INRIA, Apache-2.0
https://zenodo.org/records/21788405
https://zenodo.org/api/records/21788405/files/small.safetensors/content
SHA256 3108259779f5abfed3908a1c9eafba64d17e4a2a169368c539ecfeff04c018f3

The included kraken_ppocr/{backbone,heads,necks,network}.py modules are copied
from kraken/lib/ppocr at this pinned upstream source (Apache-2.0):
https://github.com/mittagessen/kraken/tree/0d07314ba0e6fa31d786bf3f88b20dd93597f591/kraken/lib/ppocr
Their copyright/license headers are retained. One conversion-only modification
changes `h = x.shape[2]` to `h = int(x.shape[2])` in backbone.py, making the final
pooling height constant in the ONNX graph. Input height is fixed at 96 by the
actual model metadata; width remains dynamic. No trained weights are changed.
The empty __init__.py avoids importing Kraken's unrelated training components.

Reproduction environment: Python 3.12.14, CPU PyTorch 2.14.1+cpu, NumPy 2.5.4,
ONNX 1.23.2, protobuf 7.36.2. Install these in an isolated Python environment;
the CPU PyTorch wheel index is https://download.pytorch.org/whl/cpu.
Run `python export_historical.py small.safetensors output` from this directory.
No safetensors pickle loading or untrusted Python deserialization is involved.

Expected output:
historical-recognition.onnx 12974398 bytes
SHA256 e8de47f398bd4f7943799c6bf84cc1a4093dd24ace8d23ff0a88ddc44011f57e
historical-dictionary.json 10487 bytes
SHA256 dc2070c479e38ccdce553fbfc96af26e6824333dcc969b8855875acd54f85fde

The ONNX output is [1, steps, 1623] softmax probabilities. Preprocessing is RGB,
resize preserving aspect ratio to 96px height with Lanczos3, 16 white pixels of
left/right padding, and 1 - channel/255. CTC class 0 is blank. PdfCraft uses
greedy CTC decoding with per-word canonical Unicode composition (NFC), retaining
long-s, historical abbreviations and spelling. It does not modernize text.

Validation: 62 public historical line crops from ubtue/gt-fraktur and
bertsky/mkn-kurrent-gt yielded byte-identical PyTorch/RTen transcripts before NFC.
The corpora overlap the training sources; this is conversion verification, not
an independent accuracy benchmark. See PdfCraft docs/historical-ocr.md.
