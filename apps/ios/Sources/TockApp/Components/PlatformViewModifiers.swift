import SwiftUI

extension View {
    @ViewBuilder
    func platformTextInputAutocapitalizationNever() -> some View {
        #if canImport(UIKit)
        self.textInputAutocapitalization(.never)
        #else
        self
        #endif
    }

    @ViewBuilder
    func platformTextInputAutocapitalizationCharacters() -> some View {
        #if canImport(UIKit)
        self.textInputAutocapitalization(.characters)
        #else
        self
        #endif
    }

    @ViewBuilder
    func platformInlineNavigationBarTitle() -> some View {
        #if os(macOS)
        self
        #else
        self.navigationBarTitleDisplayMode(.inline)
        #endif
    }

    @ViewBuilder
    func platformHiddenNavigationBar() -> some View {
        #if os(macOS)
        self
        #else
        self.navigationBarHidden(true)
        #endif
    }
}
