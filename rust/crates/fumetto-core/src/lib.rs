//! Il nucleo di Fumetto: aprire un volume, leggerne le pagine, decodificarle,
//! prepararle in anticipo. Niente grafica: si prova e si misura da solo.

pub mod adjust;
mod avif;
mod book;
pub mod comicinfo;
pub mod covers;
mod decode;
pub mod library;
pub mod lingua;
mod loader;
mod natural;
mod pdf;
mod progress;
mod resize;
mod settings;
pub mod upscale;

pub use book::{Book, Content, Error, is_image, read_info, title_of};
pub use decode::{DecodeError, MAX_PIXELS, Page, decode, dimensions};
pub use loader::{Decoded, Fit, Loaded, Loader, Target, decode_page, prefetch_order, to_screen};
pub use natural::natural_cmp;
pub use progress::{Progress, Saved};
pub use resize::{enlarge, resize};
pub use settings::Settings;
