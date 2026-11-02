import Tauri
import UIKit

private struct ExportRequest: Decodable {
    let path: String
}

final class PdfExport: Plugin, UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate {
    private var pending: (picker: UIDocumentPickerViewController, invoke: Invoke)?

    @objc func exportPdf(_ invoke: Invoke) throws {
        let source = URL(fileURLWithPath: try invoke.parseArgs(ExportRequest.self).path)
        DispatchQueue.main.async {
            guard self.pending == nil, let presenter = self.manager.viewController,
                  presenter.viewIfLoaded?.window != nil, presenter.presentedViewController == nil,
                  !presenter.isBeingDismissed, !presenter.isBeingPresented else {
                invoke.reject("The file picker cannot be shown right now.")
                return
            }
            guard FileManager.default.isReadableFile(atPath: source.path) else {
                invoke.reject("The staged PDF is unavailable.")
                return
            }
            let picker = UIDocumentPickerViewController(forExporting: [source], asCopy: true)
            picker.delegate = self
            picker.modalPresentationStyle = .fullScreen
            self.pending = (picker, invoke)
            presenter.present(picker, animated: true)
            picker.presentationController?.delegate = self
        }
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        guard let invoke = take(controller) else { return }
        if urls.isEmpty {
            invoke.reject("The file provider did not confirm a destination.")
        } else {
            invoke.resolve(["exported": true])
        }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        take(controller)?.resolve(["exported": false])
    }

    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        take(presentationController.presentedViewController)?.resolve(["exported": false])
    }

    private func take(_ controller: UIViewController) -> Invoke? {
        guard pending?.picker === controller else { return nil }
        defer { pending = nil }
        return pending?.invoke
    }
}

@_cdecl("init_plugin_pdf_export")
public func initPlugin() -> Plugin { PdfExport() }
