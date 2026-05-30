// TockSwift — idiomatic async/await wrappers around UniFFI bindings
//
// The UniFFI-generated API is synchronous and callback-based. This
// module wraps it into Swift-native patterns:
//
// * All vault and mutation calls dispatch to a background queue and
//   return via `async throws`.
// * Types conform to `Sendable` where appropriate.
// * Extensions add `Identifiable`, `Hashable`, and other SwiftUI-
//   friendly protocols.
//
// ## Usage
//
// ```swift
// import TockSwift
//
// let workspace = try await TockWorkspace.create(
//     path: vaultURL.path,
//     password: Data("secret".utf8)
// )
// let task = try await workspace.addTask(
//     TockNewTask(title: "Buy groceries", tags: ["errands"])
// )
// ```
//
// ## Architecture
//
// `TockWorkspace` wraps the UniFFI `Workspace` object and dispatches
// every call onto a serial `DispatchQueue` via
// `withCheckedThrowingContinuation`. This keeps the main thread free
// for UI work while the Rust/SQLite layer processes the request.
//
// A future version (per ADR-005) will use UniFFI's native async
// support backed by a Tokio current-thread runtime, eliminating the
// Swift-side queue.

import Foundation

// Re-export the generated types so consumers only need `import TockSwift`.
// Once the TockFFI generated code is in place, uncomment:
// @_exported import TockFFI

/// Actor-isolated wrapper around the UniFFI `Workspace` object.
///
/// All methods are `async throws` — they dispatch the synchronous FFI
/// call onto a background queue and return the result.
///
/// Create via ``create(path:password:)`` (new vault) or
/// ``open(path:password:)`` (existing vault).
public final class TockWorkspace: @unchecked Sendable {
    // The queue serialises calls to the underlying SQLite connection.
    private let queue = DispatchQueue(
        label: "com.tock.workspace",
        qos: .userInitiated
    )

    // Placeholder for the UniFFI Workspace handle.
    // Once TockFFI is generated, replace `Any` with the actual type:
    //     private let handle: Workspace
    private let handle: Any
    private let vaultPath: String

    private init(handle: Any, path: String) {
        self.handle = handle
        self.vaultPath = path
    }

    /// The filesystem path of the vault.
    public var path: String { vaultPath }

    // MARK: - Lifecycle

    /// Create a new vault at `path` protected by `password`.
    ///
    /// - Parameters:
    ///   - path: Filesystem path for the new vault file.
    ///   - password: Raw password bytes (zeroed after use by the Rust layer).
    /// - Returns: An open `TockWorkspace` handle.
    public static func create(path: String, password: Data) async throws -> TockWorkspace {
        // Once TockFFI is generated, replace with:
        //     let ws = try initWorkspace(path: path, password: Array(password))
        //     return TockWorkspace(handle: ws, path: path)
        fatalError("TockFFI not yet generated — run uniffi-bindgen first")
    }

    /// Open an existing vault at `path`.
    ///
    /// - Parameters:
    ///   - path: Filesystem path to the vault file.
    ///   - password: Raw password bytes.
    /// - Returns: An open `TockWorkspace` handle.
    public static func open(path: String, password: Data) async throws -> TockWorkspace {
        fatalError("TockFFI not yet generated — run uniffi-bindgen first")
    }

    /// Lock the workspace, zeroing key material.
    ///
    /// After this call, all other methods will throw.
    public func lock() async throws {
        fatalError("TockFFI not yet generated — run uniffi-bindgen first")
    }
}
