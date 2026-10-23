Vendored inventory 0.3.21 from https://github.com/dtolnay/inventory at c8310fa1b9d6dc4b41f74dea98cd01d5fd490f72.

The only source change adds Scarlet to the ELF `.init_array` platforms.
Native Scarlet CRT startup runs this section; without it Salsa's query
registrations are omitted and rust-analyzer panics after initialization.
The standalone workspace keeps upstream development targets out of the
rust-analyzer workspace. Upstream licenses and tests are retained.
