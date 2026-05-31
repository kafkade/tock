import SwiftUI
import UniformTypeIdentifiers

/// Lightweight transferable wrapper for task drag-and-drop.
///
/// Only carries the task ID — never serializes plaintext task content to the
/// pasteboard — to stay consistent with the vault encryption model.
struct TaskTransferable: Codable, Transferable, Sendable {
    let taskId: String

    static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: .tockTask)
    }
}

extension UTType {
    /// Custom UTType for in-app task drag operations.
    static let tockTask = UTType(exportedAs: "com.kafkade.tock.task")
}
