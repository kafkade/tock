#if os(iOS)
import Foundation
import LinkPresentation
import UniformTypeIdentifiers

// MARK: - Share Item Extractor

/// Extracts structured `ShareContent` from `NSItemProvider` attachments.
///
/// Processes shared items by UTI type priority: URLs first (with optional
/// metadata fetch), then plain text, then images and files. Designed for
/// use in a share extension's `NSExtensionContext`, but also callable
/// from previews with synthetic providers.
///
/// **Note:** This runs in the main app target for development. When moved
/// to a real share extension target, it will use the same extraction logic
/// but access the vault via App Group instead of in-process CoreClient.
@MainActor
final class ShareItemExtractor {

    /// Extract content from an array of item providers.
    ///
    /// Processes the first provider that matches a supported type.
    /// Returns `.empty` if no supported content is found.
    static func extract(from providers: [NSItemProvider]) async -> ShareContent {
        for provider in providers {
            if let content = await extractURL(from: provider) {
                return content
            }
            if let content = await extractText(from: provider) {
                return content
            }
            if provider.hasItemConformingToTypeIdentifier(UTType.image.identifier) {
                return ShareContent(
                    kind: .image,
                    title: provider.suggestedName ?? "Shared image",
                    notes: nil,
                    attachmentName: provider.suggestedName,
                    attachmentTypeIdentifier: UTType.image.identifier
                )
            }
            if provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
                return ShareContent(
                    kind: .file,
                    title: provider.suggestedName ?? "Shared file",
                    notes: nil,
                    attachmentName: provider.suggestedName,
                    attachmentTypeIdentifier: UTType.fileURL.identifier
                )
            }
        }
        return .empty
    }

    // MARK: - URL extraction

    private static func extractURL(from provider: NSItemProvider) async -> ShareContent? {
        guard provider.hasItemConformingToTypeIdentifier(UTType.url.identifier) else {
            return nil
        }

        let url: URL? = await withCheckedContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.url.identifier) { item, _ in
                if let url = item as? URL {
                    continuation.resume(returning: url)
                } else if let data = item as? Data, let url = URL(dataRepresentation: data, relativeTo: nil) {
                    continuation.resume(returning: url)
                } else {
                    continuation.resume(returning: nil)
                }
            }
        }

        guard let url else { return nil }

        // Use URL as both title fallback and note. Title may be updated
        // asynchronously by the view via metadata fetch.
        let host = url.host() ?? url.absoluteString
        return ShareContent(
            kind: .url,
            title: host,
            notes: url.absoluteString,
            url: url
        )
    }

    // MARK: - Text extraction

    private static func extractText(from provider: NSItemProvider) async -> ShareContent? {
        guard provider.hasItemConformingToTypeIdentifier(UTType.plainText.identifier) else {
            return nil
        }

        let text: String? = await withCheckedContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.plainText.identifier) { item, _ in
                if let string = item as? String {
                    continuation.resume(returning: string)
                } else if let data = item as? Data {
                    continuation.resume(returning: String(data: data, encoding: .utf8))
                } else {
                    continuation.resume(returning: nil)
                }
            }
        }

        guard let text, !text.isEmpty else { return nil }

        let lines = text.components(separatedBy: .newlines)
        let title = lines.first?.trimmingCharacters(in: .whitespaces) ?? text
        let notes = lines.count > 1
            ? lines.dropFirst().joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            : nil

        return ShareContent(
            kind: .text,
            title: String(title.prefix(200)),
            notes: notes
        )
    }
}

// MARK: - URL Metadata Fetcher

/// Asynchronously fetches a page title for a URL using LPMetadataProvider.
///
/// Returns `nil` if the fetch times out (2 seconds) or fails. The share
/// extension UI shows the form immediately and updates the title field
/// if metadata arrives in time.
@MainActor
final class URLMetadataFetcher {

    static func fetchTitle(for url: URL) async -> String? {
        let provider = LPMetadataProvider()

        return await withTaskGroup(of: String?.self) { group in
            group.addTask {
                do {
                    let metadata = try await provider.startFetchingMetadata(for: url)
                    return metadata.title
                } catch {
                    return nil
                }
            }

            // Timeout after 2 seconds
            group.addTask {
                try? await Task.sleep(for: .seconds(2))
                return nil
            }

            // Return whichever finishes first
            for await result in group {
                if let title = result {
                    group.cancelAll()
                    return title
                }
            }
            group.cancelAll()
            return nil
        }
    }
}
#endif
