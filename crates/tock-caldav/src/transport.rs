//! `CalDAV` transport trait and types.
//!
//! The trait defines the HTTP operations needed for `CalDAV` sync.
//! Implementations live in platform-specific crates (e.g. `tock-cli`
//! uses `reqwest`). This keeps `tock-caldav` free of I/O dependencies.

use crate::Error;

/// A `CalDAV` resource as returned by collection listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DavResource {
    /// Resource href (relative URL path).
    pub href: String,
    /// Current `ETag` (opaque version identifier).
    pub etag: Option<String>,
}

/// Result of a PUT operation.
#[derive(Clone, Debug)]
pub struct PutResult {
    /// New `ETag` assigned by the server.
    pub etag: Option<String>,
    /// Href of the created/updated resource.
    pub href: String,
}

/// `CalDAV` collection metadata.
#[derive(Clone, Debug)]
pub struct CollectionInfo {
    /// Collection href.
    pub href: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Sync token for incremental sync (if supported).
    pub sync_token: Option<String>,
    /// `CTag` (collection-level change indicator).
    pub ctag: Option<String>,
}

/// Changes detected via sync-collection REPORT or full scan.
#[derive(Clone, Debug)]
pub struct SyncChanges {
    /// Resources that are new or modified.
    pub changed: Vec<DavResource>,
    /// Hrefs that have been deleted on the server.
    pub deleted: Vec<String>,
    /// New sync token (if incremental sync was used).
    pub new_sync_token: Option<String>,
}

/// `CalDAV` transport abstraction.
///
/// Implementations handle HTTP (PROPFIND, REPORT, GET, PUT, DELETE).
/// This trait is object-safe for use with `dyn CalDavTransport`.
pub trait CalDavTransport: Send + Sync {
    /// Discover the `CalDAV` principal and calendar home URL.
    ///
    /// # Errors
    /// Transport/HTTP errors.
    fn discover(&self, base_url: &str) -> Result<String, Error>;

    /// List resources in a collection (PROPFIND depth=1).
    /// Returns `(href, etag)` pairs for each resource.
    ///
    /// # Errors
    /// Transport/HTTP errors.
    fn list_collection(&self, collection_url: &str) -> Result<Vec<DavResource>, Error>;

    /// Get collection info (display name, sync-token, ctag).
    ///
    /// # Errors
    /// Transport/HTTP errors.
    fn collection_info(&self, collection_url: &str) -> Result<CollectionInfo, Error>;

    /// Fetch a single resource body (GET).
    ///
    /// # Errors
    /// Transport/HTTP errors.
    fn get_resource(&self, href: &str) -> Result<String, Error>;

    /// Fetch multiple resources by href (multiget REPORT).
    /// Returns `(href, body)` pairs.
    ///
    /// # Errors
    /// Transport/HTTP errors.
    fn multiget(
        &self,
        collection_url: &str,
        hrefs: &[&str],
    ) -> Result<Vec<(String, String)>, Error>;

    /// PUT a resource. If `etag` is `Some`, uses `If-Match` for
    /// conditional update. If `None`, creates a new resource.
    ///
    /// # Errors
    /// - [`Error::EtagConflict`] on 412 Precondition Failed.
    /// - Other transport/HTTP errors.
    fn put_resource(
        &self,
        href: &str,
        body: &str,
        content_type: &str,
        etag: Option<&str>,
    ) -> Result<PutResult, Error>;

    /// DELETE a resource with optional `ETag` precondition.
    ///
    /// # Errors
    /// Transport/HTTP errors.
    fn delete_resource(&self, href: &str, etag: Option<&str>) -> Result<(), Error>;

    /// Sync-collection REPORT (RFC 6578) for incremental sync.
    /// If the server doesn't support it, returns `Err(Error::Unsupported)`.
    ///
    /// # Errors
    /// Transport/HTTP errors.
    fn sync_collection(&self, collection_url: &str, sync_token: &str)
    -> Result<SyncChanges, Error>;
}
