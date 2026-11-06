use super::*;
use bar::{Look, hud_buttons};

fn book(pages: usize) -> BookInfo<'static> {
    BookInfo {
        title: "Nebbia sul Porto v03",
        folio: "24".into(),
        pages,
        here: 23,
        modes: vec!["pagina singola"],
        margins: (600.0, 600.0),
        zoom: Some(72),
        double: false,
        strip: false,
        manga: false,
        cover_alone: false,
        bookmarked: false,
        bookmarks: &[],
        trim: false,
        slideshow: false,
        next: None,
        prev: None,
    }
}

fn ctx<'a>(b: Option<BookInfo<'a>>, recent: &'a [Recent]) -> Context<'a> {
    static SETTINGS: std::sync::LazyLock<fumetto_core::Settings> = std::sync::LazyLock::new(Default::default);
    Context {
        view: (1920.0, 1080.0),
        scale: 1.0,
        book: b,
        recent,
        hud: false,
        fullscreen: false,
        shelf: None,
        thumbs: None,
        settings: &SETTINGS,
        keys: Keymap::defaults(),
        lens: None,
    }
}

#[test]
fn i_tasti_mostrati_sono_quelli_scelti() {
    let mut chosen = std::collections::BTreeMap::new();
    crate::keys::assign(&mut chosen, Bind::Double, &crate::keys::Combo::parse("P").unwrap());
    let keys = Keymap::new(&chosen);
    let c = Context { keys: &keys, ..ctx(Some(book(10)), &[]) };
    let rows = main_rows(&c);
    let double = rows.iter().find_map(|r| match r {
        Row::Item { key, cmd: Command::Act(Action::ToggleDouble), .. } => Some(key.clone()),
        _ => None,
    });
    assert_eq!(double.as_deref(), Some("P"));
}

#[test]
fn le_impostazioni_si_aprono_e_un_clic_fuori_le_chiude() {
    let mut ui = Ui::new();
    let c = ctx(Some(book(10)), &[]);
    ui.toggle_prefs();
    assert!(ui.modal());
    assert_eq!(ui.click(300.0, 500.0, &c), Handled::Yes(Some(Command::Act(Action::ToggleSettings))));
}

#[test]
fn vai_a_pagina_si_scrive_e_si_conferma() {
    let mut ui = Ui::new();
    let c = ctx(Some(book(212)), &[]);
    ui.ask_page(23, 212, &[]);
    for d in [1, 4, 7] {
        ui.key(Key::Digit(d), &c);
    }
    ui.key(Key::Digit(9), &c); // oltre le cifre del volume: ignorata
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::GoTo(146)))));
    assert!(!ui.modal());
}

#[test]
fn vai_a_pagina_rifiuta_le_pagine_che_non_ci_sono() {
    let mut ui = Ui::new();
    let c = ctx(Some(book(48)), &[]);
    ui.ask_page(0, 48, &[]);
    ui.key(Key::Digit(9), &c);
    ui.key(Key::Digit(9), &c);
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(None), "99 di 48: si resta");
    assert!(ui.modal());
    ui.key(Key::Backspace, &c);
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::GoTo(8)))));
}

#[test]
fn vai_a_pagina_con_le_frecce_parte_da_qui() {
    let mut ui = Ui::new();
    let c = ctx(Some(book(212)), &[]);
    ui.ask_page(23, 212, &[]);
    ui.key(Key::Up, &c);
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::GoTo(24)))));
}

#[test]
fn la_password_di_un_pdf() {
    let mut ui = Ui::new();
    let c = ctx(None, &[]);
    ui.ask_password("protetto.pdf".into(), "Protetto".into());
    assert!(ui.modal() && ui.server_open());
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(None), "vuota non si prova");
    assert_eq!(ui.key(Key::Tab, &c), Handled::Yes(None), "un campo solo");
    ui.paste("seg");
    for ch in "reto".chars() {
        ui.key(Key::Char(ch), &c);
    }
    let open = |pw: &str| Handled::Yes(Some(Command::OpenWithPassword("protetto.pdf".into(), pw.into())));
    assert_eq!(ui.key(Key::Enter, &c), open("segreto"));
    ui.password_checked(Err("Password sbagliata.".into()));
    assert!(ui.server_open(), "sbagliata: si riscrive");
    ui.key(Key::Char('x'), &c);
    assert_eq!(ui.key(Key::Enter, &c), open("x"), "la sbagliata si cancella");
    ui.password_checked(Ok(()));
    assert!(!ui.server_open());
}

#[test]
fn i_dati_di_un_volume() {
    use fumetto_core::edits::VolumeEdit;
    let mut ui = Ui::new();
    let c = ctx(None, &[]);
    let existing = VolumeEdit { title: Some("Vecchio".into()), ..VolumeEdit::default() };
    let initial = ["Vecchio".to_owned(), String::new(), String::new()];
    ui.edit_volume("/f/scan_03.cbz".into(), "scan_03.cbz".into(), initial, existing, None);
    assert!(ui.modal() && ui.server_open());
    ui.key(Key::Tab, &c);
    ui.paste("Nebbia sul Porto");
    ui.key(Key::Tab, &c);
    ui.key(Key::Char('x'), &c);
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(None), "il numero dev'essere un numero");
    ui.key(Key::Backspace, &c);
    ui.key(Key::Digit(3), &c);
    ui.key(Key::Tab, &c); // il verso
    ui.key(Key::Char('z'), &c); // qui non si scrive
    ui.key(Key::Right, &c);
    ui.key(Key::Right, &c); // come il file -> da sinistra -> da destra
    let saved = VolumeEdit {
        title: Some("Vecchio".into()), // non toccato: resta la correzione di prima
        series: Some("Nebbia sul Porto".into()),
        number: Some(Some(3)),
        right_to_left: Some(true),
    };
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::SaveVolume("/f/scan_03.cbz".into(), saved))));
    ui.server_checked(Ok(()));
    assert!(!ui.server_open());
}

#[test]
fn il_modulo_del_server() {
    let mut ui = Ui::new();
    let c = ctx(None, &[]);
    assert_eq!(ui.internal(Some(Command::AskServer), &c), None);
    assert!(ui.modal() && ui.server_open());
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(None), "senza indirizzo non si va avanti");
    ui.paste("casa:25600\n");
    ui.key(Key::Tab, &c);
    for ch in "io@casa".chars() {
        ui.key(Key::Char(ch), &c);
    }
    ui.key(Key::Down, &c);
    ui.paste("pa ss");
    ui.key(Key::Digit(7), &c);
    ui.key(Key::Backspace, &c);
    let server = fumetto_core::remote::Server {
        url: "http://casa:25600".into(),
        user: "io@casa".into(),
        password: "pa ss".into(),
    };
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::AddServer(server))));
    ui.key(Key::Char('x'), &c);
    ui.server_checked(Err("nome o password sbagliati".into()));
    assert!(ui.server_open(), "se non va, si corregge");
    ui.server_checked(Ok(()));
    assert!(!ui.server_open());
    ui.internal(Some(Command::AskServer), &c);
    assert_eq!(ui.key(Key::Escape, &c), Handled::Yes(None));
    assert!(!ui.server_open());
    assert_eq!(ui.key(Key::Tab, &c), Handled::No, "fuori dal modulo Tab non e' dell'interfaccia");
}

#[test]
fn il_menu_salta_titoli_e_separatori() {
    let recent = [Recent { path: "a.cbz".into(), title: "A".into(), place: "3 / 10".into() }];
    let mut ui = Ui::new();
    let c = ctx(Some(book(10)), &recent);
    ui.open_menu(100.0, 100.0, &c);
    ui.key(Key::Down, &c); // Apri
    ui.key(Key::Down, &c); // Apri cartella
    ui.key(Key::Down, &c); // Libreria
    ui.key(Key::Down, &c); // Aggiungi un server
    ui.key(Key::Down, &c); // oltre separatore e titolo: il volume recente
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Open("a.cbz".into()))));
    ui.open_menu(100.0, 100.0, &c);
    ui.key(Key::Up, &c); // dal fondo: Chiudi
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::Close))));
}

/// Il menu lungo va in due colonne: un clic nella seconda sceglie una sua voce.
#[test]
fn il_menu_in_due_colonne() {
    let mut ui = Ui::new();
    let c = ctx(Some(book(10)), &[]);
    ui.open_menu(100.0, 100.0, &c);
    let m = ui.menu.as_ref().unwrap();
    let (frame, spots) = m.frame(&c);
    assert!((frame.w - 2.0 * super::menu::MENU_W).abs() < 1.0, "due colonne: {}", frame.w);
    let double =
        m.rows.iter().position(|r| matches!(r, Row::Item { cmd: Command::Act(Action::ToggleDouble), .. })).unwrap();
    let (x, y) = spots[double];
    assert!(x > frame.x + super::menu::MENU_W - 1.0, "la seconda colonna");
    assert_eq!(ui.click(x + 50.0, y + 10.0, &c), Handled::Yes(Some(Command::Act(Action::ToggleDouble))));
}

#[test]
fn i_segnalibri_nel_menu() {
    let marks = [2, 7];
    let mut ui = Ui::new();
    let c = ctx(Some(BookInfo { bookmarks: &marks, ..book(10) }), &[]);
    ui.open_menu(100.0, 100.0, &c);
    let rows = &ui.menu.as_ref().unwrap().rows;
    let jumps: Vec<&Command> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Recent { cmd: cmd @ Command::Act(Action::GoTo(_)), .. } => Some(cmd),
            _ => None,
        })
        .collect();
    assert_eq!(jumps, [&Command::Act(Action::GoTo(2)), &Command::Act(Action::GoTo(7))]);
}

#[test]
fn la_didascalia_compare_e_poi_svanisce() {
    let mut ui = Ui::new();
    let c = ctx(Some(book(10)), &[]);
    let t0 = Instant::now();
    assert_eq!(ui.wake(t0), None, "all'inizio non c'e' niente da ridisegnare");
    ui.motion(10.0, 10.0, &c, t0);
    assert_eq!(ui.wake(t0), Some(t0), "sta comparendo");
    let still = t0 + Duration::from_millis(500);
    assert_eq!(ui.wake(still), Some(t0 + CAPTION_HOLD), "ferma fino alla scadenza");
    let gone = t0 + CAPTION_HOLD + Duration::from_secs(1);
    assert!(ui.scene(&c, gone, &mut fumetto_render::Estimate).is_empty());
    assert_eq!(ui.wake(gone), None);
}

#[test]
fn un_titolo_troppo_lungo_si_taglia_con_i_puntini() {
    let t = Text::new("Il Guardiano del Faro e la Lanterna Perduta", Face::Serif, 20.0, INK);
    let cut = fit(&mut fumetto_render::Estimate, t, 200.0);
    assert!(cut.text.ends_with('\u{2026}'), "{}", cut.text);
    assert!(fumetto_render::Estimate.width(&cut) <= 200.0);
    let short = fit(&mut fumetto_render::Estimate, Text::new("Lanterne", Face::Serif, 20.0, INK), 200.0);
    assert_eq!(short.text, "Lanterne");
}

#[test]
fn la_barra_cambia_modo_e_non_gira_pagina() {
    let mut ui = Ui::new();
    let mut c = ctx(Some(book(10)), &[]);
    c.hud = true;
    let b = c.book.as_ref().unwrap();
    let buttons = hud_buttons(&c, b);
    let double = buttons.iter().find(|b| b.face == Look::Icon(Icon::Double)).unwrap();
    let (x, y) = (double.x + double.w / 2.0, 20.0);
    assert_eq!(ui.click(x, y, &c), Handled::Yes(Some(Command::Act(Action::ToggleDouble))));
    assert_eq!(ui.click(5.0 * 60.0, 20.0, &c), Handled::Yes(None), "fra i tasti: niente, e nessun giro di pagina");
    assert_eq!(ui.click(x, 500.0, &c), Handled::No, "sotto la barra: le pagine");
}

#[test]
fn la_percentuale_apre_i_livelli_di_zoom() {
    let mut ui = Ui::new();
    let mut c = ctx(Some(book(10)), &[]);
    c.hud = true;
    let b = c.book.as_ref().unwrap();
    let zoom = hud_buttons(&c, b).into_iter().find(|b| b.id() == Hover::Zoom).unwrap();
    ui.click(zoom.x + 5.0, 20.0, &c);
    assert!(ui.modal());
    ui.key(Key::Down, &c);
    ui.key(Key::Down, &c);
    assert_eq!(ui.key(Key::Enter, &c), Handled::Yes(Some(Command::Act(Action::ZoomTo(Zoom::Width)))));
}

#[test]
fn nella_doppia_pagina_lo_zoom_e_spento() {
    let mut c = ctx(Some(BookInfo { zoom: None, double: true, ..book(10) }), &[]);
    c.hud = true;
    let b = c.book.as_ref().unwrap();
    let buttons = hud_buttons(&c, b);
    assert!(buttons.iter().filter(|b| matches!(b.face, Look::Icon(Icon::Plus | Icon::Minus))).all(|b| b.cmd.is_none()));
}

#[test]
fn nella_galleria_vuota_un_clic_apre() {
    let recent = [Recent { path: "a.cbz".into(), title: "A".into(), place: "letto".into() }];
    let mut ui = Ui::new();
    let c = ctx(None, &recent);
    let y = super::welcome::welcome_rows_top(&c) + 10.0;
    assert_eq!(ui.click(960.0, y, &c), Handled::Yes(Some(Command::Open("a.cbz".into()))));
    assert_eq!(ui.click(960.0, 20.0, &c), Handled::Yes(Some(Command::Act(Action::Open))));
}

#[test]
fn gli_ultimi_letti_stanno_sopra_i_tasti() {
    let recent: Vec<Recent> = (0..8)
        .map(|i| Recent { path: format!("{i}.cbz").into(), title: format!("{i}"), place: "letto".into() })
        .collect();
    let tall = ctx(None, &recent);
    assert_eq!(super::recent_shown(&tall), 6, "al massimo sei");
    assert_eq!(super::welcome_covers(&tall).len(), 6, "ognuno con la sua copertina");
    let short = Context { view: (1100.0, 700.0), scale: 1.5, ..ctx(None, &recent) };
    let n = super::recent_shown(&short);
    assert!(n < 6, "in una finestra bassa, meno righe");
    let last = super::welcome::welcome_rows_top(&short) + n as f32 * super::WELCOME_ROW * 1.5;
    assert!(last <= 700.0 - 56.0 * 1.5, "l'ultima finisce sopra i tasti in fondo");
    // le frecce scelgono solo fra quelli mostrati
    let mut ui = Ui::new();
    for _ in 0..n {
        ui.key(Key::Down, &short);
    }
    assert_eq!(ui.selected, Some(0), "dopo l'ultimo si torna al primo");
}
