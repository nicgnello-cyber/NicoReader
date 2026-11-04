use super::*;
use std::collections::HashMap;

/// Pagine finte: quali sono pronte e per quale misura.
#[derive(Default)]
struct Fake(HashMap<usize, Target>);

impl Pages for Fake {
    fn prepared_for(&self, i: usize) -> Option<Target> {
        self.0.get(&i).copied()
    }
}

/// Un volume di `n` pagine verticali 1000x1500, finestra 1920x1080.
fn reader(n: usize) -> Reader {
    let mut r = Reader::new(n, 0);
    r.set_view(1920, 1080);
    for i in 0..n {
        r.known(i, (1000, 1500));
    }
    r
}

fn ready_all(r: &Reader, pages: impl IntoIterator<Item = usize>) -> Fake {
    Fake(pages.into_iter().map(|i| (i, r.target())).collect())
}

fn now() -> Instant {
    Instant::now()
}

#[test]
fn dopo_un_salto_prima_solo_la_pagina_a_schermo() {
    let mut r = reader(50);
    let (_, order) = r.prefetch(&Fake::default());
    assert_eq!(order, [0], "all'avvio si prepara solo la pagina da guardare");
    let pages = ready_all(&r, [0]);
    let (_, order) = r.prefetch(&pages);
    assert_eq!(order, [1, 2, 3, 4], "arrivata la prima, le successive");
}

#[test]
fn la_fine_del_volume() {
    let mut r = reader(3);
    assert!(!r.at_end());
    r.act(Action::Next, now());
    r.act(Action::Next, now());
    assert!(r.at_end(), "l'ultima pagina");
    r.act(Action::ToggleDouble, now());
    r.act(Action::GoTo(1), now());
    assert!(!r.at_end(), "in doppia pagina: 1 e 2 insieme, poi la 3");
    r.act(Action::Next, now());
    assert!(r.at_end());
    r.act(Action::ToggleStrip, now());
    r.act(Action::First, now());
    assert!(!r.at_end(), "il nastro in cima");
    r.act(Action::Scroll(1e6), now());
    while r.advance_glide(0.05) {}
    assert!(r.at_end(), "il nastro in fondo");
}

#[test]
fn le_pagine_pronte_non_si_richiedono() {
    let mut r = reader(50);
    let pages = ready_all(&r, [0, 1, 2]);
    let (_, order) = r.prefetch(&pages);
    assert_eq!(order, [3, 4]);
}

#[test]
fn chi_sfoglia_in_fretta_prepara_piu_lontano() {
    let mut r = reader(50);
    let t = now();
    r.act(Action::Next, t);
    r.act(Action::Next, t + Duration::from_millis(40));
    let pages = ready_all(&r, [2]);
    let (_, order) = r.prefetch(&pages);
    assert_eq!(order.len(), 12 + 2, "12 avanti e 2 indietro");
}

#[test]
fn niente_lampo_nero_se_la_pagina_nuova_non_e_pronta() {
    let mut r = reader(10);
    let pages = ready_all(&r, [0]);
    r.drawn(&pages);
    r.act(Action::Next, now());
    let items = r.layout(&pages);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].page, 0, "resta la pagina di prima");
    assert!(!r.ready(&pages));
}

#[test]
fn pagina_illeggibile_non_blocca_il_lettore() {
    let mut r = Reader::new(10, 0);
    r.set_view(1920, 1080);
    r.failed(0);
    let (_, order) = r.prefetch(&Fake::default());
    assert_eq!(order, [1, 2, 3, 4], "si va oltre, senza richiederla");
    assert!(r.ready(&Fake::default()), "a schermo va lei (nera), non la pagina di prima");
    assert!(r.title("x").ends_with(t("pagina illeggibile", "unreadable page")));
    r.act(Action::ToggleDouble, now());
    r.known(1, (1000, 1500));
    let pages = ready_all(&r, [1]);
    let items = r.layout(&pages);
    assert_eq!(items.iter().map(|it| it.page).collect::<Vec<_>>(), [0, 1], "la compagna resta al suo posto");
}

#[test]
fn vai_a_pagina_nella_doppia_parte_dalla_coppia() {
    let mut r = reader(10);
    r.act(Action::ToggleDouble, now());
    r.act(Action::GoTo(6), now());
    assert_eq!((r.here(), r.folio()), (6, "7\u{2013}8".to_string()), "la coppia che contiene la pagina 7");
    r.act(Action::GoTo(99), now());
    assert_eq!(r.here(), 8, "oltre la fine: l'ultima coppia");
    let (_, order) = r.prefetch(&Fake::default());
    assert_eq!(order, [8, 9], "dopo il salto, solo le pagine a schermo");
}

#[test]
fn zoom_a_larghezza_e_a_pixel_reali() {
    let mut r = reader(10); // pagine 1000x1500 in 1920x1080: intera e' 720 di larghezza
    assert_eq!(r.zoom_percent(), Some(72));
    r.act(Action::ZoomTo(Zoom::Actual), now());
    assert_eq!(r.zoom_percent(), Some(100));
    r.act(Action::ZoomTo(Zoom::Width), now());
    assert_eq!(r.zoom_percent(), Some(192), "larga quanto la finestra");
    r.act(Action::ZoomTo(Zoom::Page), now());
    assert_eq!(r.zoom_percent(), Some(72));
    r.act(Action::ZoomOut, now());
    assert_eq!(r.zoom_percent(), Some(72), "mai piu' piccola della pagina intera");
    r.act(Action::ToggleDouble, now());
    assert_eq!(r.zoom_percent(), None, "nella doppia pagina lo zoom non si regola");
}

#[test]
fn nel_nastro_lo_zoom_e_la_larghezza() {
    let mut r = reader(10);
    r.act(Action::ToggleStrip, now());
    let before = r.zoom_percent().unwrap();
    r.act(Action::ZoomIn, now());
    assert!(r.zoom_percent().unwrap() > before);
    r.act(Action::ZoomTo(Zoom::Actual), now());
    assert_eq!(r.zoom_percent(), Some(100));
}

#[test]
fn doppia_pagina_coppie_e_copertina() {
    let mut r = reader(7);
    r.act(Action::ToggleDouble, now());
    assert_eq!(r.group_of(0), (0, 2));
    assert_eq!(r.group_of(5), (4, 2));
    assert_eq!(r.group_of(6), (6, 1), "l'ultima dispari sta da sola");
    r.act(Action::ToggleCover, now());
    assert_eq!(r.group_of(0), (0, 1));
    assert_eq!(r.group_of(2), (1, 2));
}

#[test]
fn doppia_pagina_tavola_orizzontale_da_sola() {
    let mut r = reader(6);
    r.known(2, (3000, 1500)); // una tavola doppia gia' unita
    r.act(Action::ToggleDouble, now());
    assert_eq!(r.group_of(0), (0, 2));
    assert_eq!(r.group_of(2), (2, 1));
    assert_eq!(r.group_of(3), (3, 2), "le coppie ripartono dopo");
    r.act(Action::Next, now());
    assert_eq!(r.page, 2);
    r.act(Action::Next, now());
    assert_eq!(r.page, 3);
    r.act(Action::Prev, now());
    assert_eq!(r.page, 2);
}

#[test]
fn doppia_pagina_manga_da_destra() {
    let mut r = reader(4);
    r.act(Action::ToggleDouble, now());
    let pages = ready_all(&r, 0..4);
    let normal = r.layout(&pages);
    assert_eq!((normal[0].page, normal[1].page), (0, 1));
    assert_eq!(normal[0].x + normal[0].w, normal[1].x, "affiancate senza fessura");
    r.act(Action::ToggleManga, now());
    let manga = r.layout(&pages);
    assert_eq!((manga[0].page, manga[1].page), (1, 0), "la prima a destra");
}

/// Ingrandita tre volte la pagina supera la finestra in entrambe le
/// direzioni, e un punto non troppo vicino al bordo resta dov'era. (Se la
/// pagina restasse piu' stretta della finestra starebbe centrata, e il
/// punto si sposterebbe: giustamente.)
#[test]
fn zoom_tiene_fermo_il_punto_sotto_il_puntatore() {
    let mut r = reader(3);
    let (x, y, w, h) = r.single_rect(0).unwrap();
    let (px, py) = (x + w * 0.45, y + h * 0.45);
    r.act(Action::Zoom { factor: 3.0, x: px, y: py }, now());
    let (x2, y2, w2, h2) = r.single_rect(0).unwrap();
    assert!(((px - x2) / w2 - 0.45).abs() < 1e-3 && ((py - y2) / h2 - 0.45).abs() < 1e-3, "il punto e' scivolato via");
    assert!(r.zoomed());
}

#[test]
fn zoom_non_mostra_mai_il_vuoto_oltre_i_bordi() {
    let mut r = reader(3);
    r.act(Action::Zoom { factor: 3.0, x: 960.0, y: 540.0 }, now());
    r.act(Action::Pan(-99_999.0, -99_999.0), now());
    let (x, y, w, h) = r.single_rect(0).unwrap();
    assert_eq!(x + w, 1920.0, "bordo destro della pagina sul bordo della finestra");
    assert_eq!(y + h, 1080.0);
    r.act(Action::ZoomReset, now());
    assert!(!r.zoomed());
}

#[test]
fn girando_pagina_ingrandita_si_riparte_dall_alto() {
    let mut r = reader(3);
    r.act(Action::Zoom { factor: 2.0, x: 960.0, y: 540.0 }, now());
    r.act(Action::Pan(0.0, -99_999.0), now());
    r.act(Action::Next, now());
    assert_eq!(r.single_rect(1).unwrap().1, 0.0, "la pagina nuova parte dal suo bordo alto");
}

#[test]
fn allargare_il_nastro_non_sposta_il_punto_di_lettura() {
    let mut r = reader(10);
    r.act(Action::ToggleStrip, now());
    r.strip = Strip { anchor: 3, offset: 500.0 };
    let before = r.strip.offset / r.strip_height(3);
    r.act(Action::StripWider(1.5), now());
    let after = r.strip.offset / r.strip_height(3);
    assert!((before - after).abs() < 1e-4, "da {before} a {after}");
}

#[test]
fn riprende_dove_si_era_rimasti() {
    let mut r = reader(30);
    r.act(Action::ToggleStrip, now());
    r.strip = Strip { anchor: 12, offset: r.strip_height(12) * 0.4 };
    r.manga = true;
    let saved = r.snapshot();

    let mut again = Reader::new(30, 0);
    again.restore(&saved);
    again.set_view(1920, 1080);
    assert_eq!((again.mode, again.here(), again.manga), (Mode::Strip, 12, true));
    again.known(12, (1000, 1500));
    let frac = again.strip.offset / again.strip_height(12);
    assert!((frac - 0.4).abs() < 1e-3, "punto dentro la pagina perso: {frac}");
}

/// Il difetto trovato dalla prova: dopo la ripresa la pagina d'ancora non e'
/// ancora decodificata, e la stima della sua altezza cambia man mano che
/// arrivano le altre. La posizione deve restare "il 40% della pagina", non
/// un numero di pixel calcolato sulla prima stima.
#[test]
fn ripresa_nel_nastro_con_stime_che_cambiano() {
    let saved = Saved { page: 12, pages: 30, mode: "nastro".into(), strip_offset: 0.4, ..Default::default() };
    let mut r = Reader::new(30, 0);
    r.restore(&saved);
    r.set_view(1920, 1080);
    // arrivano prima altre pagine, orizzontali: la stima cambia molto
    for i in [13, 14, 15] {
        r.known(i, (3840, 2160));
    }
    r.known(12, (3840, 2160));
    let s = r.strip_now();
    let frac = s.offset / r.strip_height(12);
    assert_eq!(s.anchor, 12);
    assert!((frac - 0.4).abs() < 1e-3, "si riparte dal punto sbagliato: {frac}");
}

#[test]
fn volume_accorciato_resta_nei_limiti() {
    let saved = Saved { page: 500, pages: 600, mode: "pagina".into(), ..Default::default() };
    let mut r = Reader::new(20, 0);
    r.restore(&saved);
    assert_eq!(r.here(), 19);
}

#[test]
fn riconosce_i_webtoon() {
    let strips = [(800, 12_000), (800, 11_500), (800, 3000), (720, 900)];
    assert_eq!(webtoon_width(&strips), Some(800), "la mediana e' una striscia");
    assert_eq!(webtoon_width(&[(1000, 1500); 5]), None, "pagine di fumetto");
    assert_eq!(webtoon_width(&[(800, 12_000); 2]), None, "due pagine non bastano");
}

/// La larghezza di partenza del nastro: 1,5 volte le strisce, mai oltre
/// il 60% della finestra. Strisce da 720 in 1920: 1080 invece di 1152.
#[test]
fn il_nastro_parte_dalla_misura_delle_strisce() {
    let sizes: Vec<(usize, (u32, u32))> = (0..5).map(|i| (i, (720, 10_000))).collect();
    let mut r = Reader::new(40, 0);
    r.adopt_webtoon(&sizes); // all'apertura la finestra non ha ancora la sua misura
    assert_eq!(r.mode, Mode::Strip);
    r.set_view(1920, 1080);
    assert_eq!(r.strip_width(), 1080);
    let mut wide = Reader::new(40, 0);
    wide.adopt_webtoon(&(0..5).map(|i| (i, (1600, 10_000))).collect::<Vec<_>>());
    wide.set_view(1920, 1080);
    assert_eq!(wide.strip_width(), 1152, "strisce grandi: il 60% della finestra");
    r.act(Action::StripWider(1.2), now());
    r.act(Action::ZoomTo(Zoom::Page), now());
    assert_eq!(r.strip_width(), 1080, "0 torna alla larghezza naturale");
    assert_eq!(r.snapshot().strip_width, 1080.0 / 1920.0);
}

#[test]
fn girare_cambia_il_bersaglio_e_si_ricorda() {
    let mut r = reader(4); // 1000x1500
    let before = r.single_rect(0).unwrap();
    r.act(Action::Rotate(true), now());
    assert_eq!(r.target().rotation, 90);
    assert_eq!(r.single_rect(0).unwrap(), before, "finche' la pagina girata non arriva, resta quella di prima");
    r.known(0, (1500, 1000));
    let after = r.single_rect(0).unwrap();
    assert!(after.2 > after.3 && before.2 < before.3, "ora e' orizzontale: {after:?}");
    let saved = r.snapshot();
    assert_eq!(saved.rotation, 90);
    r.act(Action::Rotate(false), now());
    assert_eq!(r.target().rotation, 0);
    let mut again = Reader::new(4, 0);
    again.restore(&saved);
    assert_eq!(again.target().rotation, 90);
}

#[test]
fn segnalibri() {
    let mut r = reader(10);
    r.act(Action::GoTo(4), now());
    r.act(Action::ToggleBookmark, now());
    r.act(Action::GoTo(1), now());
    r.act(Action::ToggleBookmark, now());
    assert_eq!(r.bookmarks(), [1, 4], "in ordine");
    assert!(r.bookmarked());
    r.act(Action::ToggleBookmark, now());
    assert_eq!(r.bookmarks(), [4], "di nuovo: tolto");
    let mut again = Reader::new(10, 0);
    again.restore(&r.snapshot());
    assert_eq!(again.bookmarks(), [4]);
}

#[test]
fn il_rifilo_non_tocca_il_nastro() {
    let mut r = reader(10);
    r.act(Action::ToggleTrim, now());
    assert!(r.target().trim);
    assert!(r.modes().contains(&t("rifilata", "trimmed")));
    r.act(Action::ToggleStrip, now());
    assert!(!r.target().trim, "nel nastro il bianco fra le vignette fa parte del disegno");
}

#[test]
fn dal_nastro_alla_doppia_pagina_si_resta_dove_si_era() {
    let mut r = reader(40);
    r.act(Action::ToggleStrip, now());
    r.act(Action::Scroll(20_000.0), now());
    while r.advance_glide(0.05) {}
    let here = r.here();
    assert!(here > 5, "il nastro e' sceso: {here}");
    r.act(Action::ToggleDouble, now());
    assert_eq!(r.mode, Mode::Double);
    assert_eq!(r.group_of(r.here()), r.group_of(here), "la coppia della pagina che si leggeva");
}
