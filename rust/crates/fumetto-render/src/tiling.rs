//! Come si taglia una pagina in tasselli orizzontali.
//!
//! Una striscia di webtoon puo' superare l'altezza massima di una texture
//! (8192 garantiti, 16384 sulle schede comuni). Si divide in tasselli, e ogni
//! tassello ripete un paio di righe dei vicini: l'interpolazione bicubica ne
//! legge due oltre il bordo, e senza quelle righe si vedrebbe la giuntura.

use std::ops::Range;

/// Righe utili per tassello: ben sotto ogni limite, e abbastanza piccolo da
/// disegnare solo i tasselli visibili quando si scorre una striscia lunga.
pub const TILE_ROWS: u32 = 4096;

/// Righe ripetute sopra e sotto: quante ne legge Catmull-Rom oltre il bordo.
pub const MARGIN: u32 = 2;

/// Un tassello: righe `stored` caricate in texture, di cui `core` sono sue.
#[derive(Clone, Debug, PartialEq)]
pub struct TileRows {
    pub stored: Range<u32>,
    pub core: Range<u32>,
}

pub fn tile_rows(height: u32) -> Vec<TileRows> {
    (0..height.div_ceil(TILE_ROWS))
        .map(|k| {
            let core = k * TILE_ROWS..((k + 1) * TILE_ROWS).min(height);
            let stored = core.start.saturating_sub(MARGIN)..(core.end + MARGIN).min(height);
            TileRows { stored, core }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagina_normale_un_tassello() {
        assert_eq!(tile_rows(3000), [TileRows { stored: 0..3000, core: 0..3000 }]);
    }

    #[test]
    fn striscia_lunga_con_margini() {
        let t = tile_rows(10_000);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0], TileRows { stored: 0..4096 + MARGIN, core: 0..4096 });
        assert_eq!(t[1], TileRows { stored: 4096 - MARGIN..8192 + MARGIN, core: 4096..8192 });
        assert_eq!(t[2], TileRows { stored: 8192 - MARGIN..10_000, core: 8192..10_000 });
    }

    /// I core coprono tutte le righe, una volta sola, in ordine.
    #[test]
    fn core_contigui() {
        for h in [1, 4095, 4096, 4097, 8192, 30_001] {
            let mut next = 0;
            for t in tile_rows(h) {
                assert_eq!(t.core.start, next);
                assert!(t.stored.start <= t.core.start && t.core.end <= t.stored.end);
                next = t.core.end;
            }
            assert_eq!(next, h);
        }
    }
}
