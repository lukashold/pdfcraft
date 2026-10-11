"""Reproduce the optional historical OCR model, outside the PdfCraft build.

Contributor-original exporter, licensed MIT OR Apache-2.0.
Requires the accompanying Apache-2.0 Kraken kraken_ppocr modules.
"""
import argparse
import hashlib
import json
import pathlib
import struct

import numpy as np
import torch
from kraken_ppocr.network import build_recognizer


SOURCE_SHA256 = "3108259779f5abfed3908a1c9eafba64d17e4a2a169368c539ecfeff04c018f3"


class Export(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, image):
        logits, _ = self.model(image)
        return logits.squeeze(2).transpose(1, 2).softmax(-1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=pathlib.Path, help="Pinned Zenodo small.safetensors")
    parser.add_argument("output", type=pathlib.Path)
    args = parser.parse_args()
    data = args.source.read_bytes()
    if hashlib.sha256(data).hexdigest() != SOURCE_SHA256:
        raise ValueError("source checksum mismatch")
    size = struct.unpack("<Q", data[:8])[0]
    header = json.loads(data[8:8 + size])
    metadata = json.loads(header.pop("__metadata__")["kraken_meta"])
    prefix, config = next(iter(metadata.items()))
    assert config["variant"] == "small" and config["height"] == 96
    assert config["num_classes"] == 1623
    state = {}
    for key, tensor in header.items():
        start, end = tensor["data_offsets"]
        dtype = {"F32": "<f4", "I64": "<i8"}[tensor["dtype"]]
        array = np.frombuffer(data, dtype=dtype, offset=8 + size + start,
                              count=(end - start) // np.dtype(dtype).itemsize)
        state[key.removeprefix(prefix + ".nn.")] = torch.from_numpy(array.copy().reshape(tensor["shape"]))
    model = build_recognizer(config["variant"], config["num_classes"])
    model.load_state_dict(state, strict=True)
    model.eval()
    codec = [""] * config["num_classes"]
    for character, tokens in config["codec"].items():
        assert len(tokens) == 1
        codec[tokens[0]] = character
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "historical-dictionary.json").write_text(
        json.dumps(codec, ensure_ascii=False), encoding="utf8")
    torch.onnx.export(
        Export(model).eval(), torch.zeros(1, 3, config["height"], 512),
        args.output / "historical-recognition.onnx",
        input_names=["image"], output_names=["probabilities"],
        dynamic_axes={"image": {3: "width"}, "probabilities": {1: "steps"}},
        opset_version=17, dynamo=False,
    )
    for filename in ["historical-recognition.onnx", "historical-dictionary.json"]:
        content = (args.output / filename).read_bytes()
        print(filename, len(content), hashlib.sha256(content).hexdigest())


if __name__ == "__main__":
    main()
