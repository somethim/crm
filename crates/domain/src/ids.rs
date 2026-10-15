//! Typed row ids. Each wraps the SQLite `INTEGER PRIMARY KEY` as an `i64`.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl $name {
            /// The raw `i64` value.
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl From<i64> for $name {
            fn from(value: i64) -> Self {
                Self(value)
            }
        }

        impl From<$name> for i64 {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(
    /// Id of a row in `skus` (the shared stock and price pool).
    SkuId
);
id_type!(
    /// Id of a row in `products` (a named product that belongs to one SKU).
    ProductId
);
id_type!(
    /// Id of a row in `owners` (a client or supplier).
    OwnerId
);
id_type!(
    /// Id of a row in `orders`; shown to the user as "Nr. Porosie".
    OrderId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_order_and_serialize_as_plain_integers() {
        assert!(SkuId(1) < SkuId(2));
        assert_eq!(OrderId::from(7).get(), 7);
        assert_eq!(i64::from(ProductId(9)), 9);
        assert_eq!(OwnerId(3).to_string(), "3");
    }
}
