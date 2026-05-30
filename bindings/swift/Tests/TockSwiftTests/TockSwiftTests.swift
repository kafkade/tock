// Placeholder for TockSwift test target.
//
// Tests require the TockFFI generated bindings and a compiled Rust
// library (XCFramework). They will be enabled once the full build
// pipeline is in place. See bindings/swift/README.md for instructions.

import XCTest

final class TockSwiftTests: XCTestCase {
    func testPlaceholder() {
        // This test exists to satisfy SPM's requirement for at least
        // one test file in the target. Replace with real tests once
        // the UniFFI bindings are generated and the XCFramework is
        // available.
        XCTAssertTrue(true, "Placeholder test — replace after UniFFI bindgen")
    }
}
