import Foundation

/// Global actor serializing all calls to the Rust core via UniFFI.
///
/// Currently a placeholder — the actual UniFFI workspace handle will be
/// injected once `TockFFI` is generated. All mutations and reads are
/// serialized through this actor to match SQLite's single-writer model.
@globalActor
actor CoreActor {
    static let shared = CoreActor()

    // In production this will hold the TockWorkspace from UniFFI.
    // For now, it's unused — all calls go through CoreClient protocol.
    // private var workspace: TockWorkspace?
}
