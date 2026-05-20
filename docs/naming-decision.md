# Naming Decision

- **Selected name:** `tock`
- **Rationale:** short, rhythmic, time-evocative, evokes “tick-tock,” just 4 characters, and fits a fast CLI-first product.
- **Known conflicts:** crates.io has a `tock` crate (digital clock, ~12K downloads) and the App Store has **Tock** for restaurant reservations. Neither is in the same domain.
- **Strategy:** publish crates as `tock-app`, `tock-cli`, `tock-core`, or `tock-server` as needed, while keeping the binary name `tock`. Use **Tock — Tasks & Habits** for the App Store listing.
- **CLI alias:** recommend `tt` as the power-user alias in the README.
- **Revisit note:** the name is locked for now, but may be revisited before launch if a better option surfaces.
