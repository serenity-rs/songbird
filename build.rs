#[cfg(all(
    feature = "driver",
    not(any(feature = "rustls-marker", feature = "native-marker"))
))]
compile_error!(
    "You have the `driver` feature enabled: \
    either the `rustls` or `native` feature must be
    selected to let Songbird's driver use websockets.\n\
    - `rustls` uses Rustls, a pure Rust TLS-implemenation.\n\
    - `native` uses SChannel on Windows, Secure Transport on macOS, \
    and OpenSSL on other platforms.\n\
    If you are unsure, go with `rustls`."
);

#[cfg(all(
    feature = "twilight",
    not(any(feature = "zlib-simd", feature = "zlib-stock"))
))]
compile_error!(
    "Twilight requires you to specify a zlib backend: \
    either the `zlib-simd` or `zlib-stock` feature must be
    selected.\n\
    If you are unsure, go with `zlib-stock`."
);

fn main() {}
