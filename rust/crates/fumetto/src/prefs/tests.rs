use super::*;

const VIEW: (f32, f32) = (1920.0, 1080.0);

fn open(tab: Tab) -> (Prefs, Settings, Keymap) {
    (Prefs::open(tab), Settings::default(), Keymap::default())
}

#[test]
fn trascinare_la_luminosita() {
    let (mut p, st, km) = open(Tab::Image);
    let g = Geo::new(VIEW, 1.0);
    let rows = p.rows(&st, &km);
    let top = p.tops(&rows, &g)[0];
    let (tx, tw, ty) = g.track(top);
    // tre quarti della pista: +50
    assert_eq!(p.press(&st, &km, VIEW, 1.0, tx + tw * 0.75, ty), Some(Command::Pref(Pref::Brightness(50))));
    assert_eq!(
        p.drag(&st, &km, VIEW, 1.0, tx - 100.0),
        Some(Command::Pref(Pref::Brightness(-100))),
        "oltre il bordo: il minimo"
    );
    assert_eq!(p.click(&st, &km, VIEW, 1.0, tx, ty), Handled::Yes(None), "rilasciando non cambia altro");
}

#[test]
fn l_avviso_delle_versioni_nuove_si_spegne() {
    let (p, mut st, km) = open(Tab::Reading);
    let toggle = |p: &Prefs, st: &Settings| {
        p.rows(st, &km).into_iter().find_map(|r| match r {
            Row::Toggle { on, pref: pref @ Pref::CheckUpdates(_), .. } => Some((on, pref)),
            _ => None,
        })
    };
    assert_eq!(toggle(&p, &st), Some((true, Pref::CheckUpdates(false))), "acceso di serie");
    st.check_updates = false;
    assert_eq!(toggle(&p, &st), Some((false, Pref::CheckUpdates(true))));
}

#[test]
fn con_la_tastiera() {
    let (mut p, st, km) = open(Tab::Image);
    p.key(Key::Down, &st, &km, VIEW, 1.0); // luminosita'
    assert_eq!(p.key(Key::Right, &st, &km, VIEW, 1.0), Handled::Yes(Some(Command::Pref(Pref::Brightness(5)))));
    p.key(Key::PageDown, &st, &km, VIEW, 1.0); // Lettura
    p.key(Key::Down, &st, &km, VIEW, 1.0); // webtoon, acceso di serie
    assert_eq!(p.key(Key::Enter, &st, &km, VIEW, 1.0), Handled::Yes(Some(Command::Pref(Pref::Webtoon(false)))));
    assert_eq!(p.key(Key::Escape, &st, &km, VIEW, 1.0), Handled::Yes(Some(Command::Act(Action::ToggleSettings))));
}

#[test]
fn un_tasto_nuovo_per_un_comando() {
    let (mut p, st, km) = open(Tab::Keys);
    let g = Geo::new(VIEW, 1.0);
    let rows = p.rows(&st, &km);
    let i = rows.iter().position(|r| matches!(r, Row::Key { bind: Bind::Lens, .. })).unwrap();
    let top = p.tops(&rows, &g)[i];
    p.click(&st, &km, VIEW, 1.0, g.x + 60.0, top + 10.0);
    assert_eq!(p.capturing(), Some(Bind::Lens));
    let k = Combo::parse("Z").unwrap();
    assert_eq!(p.captured(Some(k.clone())), Some(Command::Pref(Pref::Bind(Bind::Lens, k))));
    assert_eq!(p.capturing(), None);
}

#[test]
fn la_presentazione_a_passi() {
    let (mut p, st, km) = open(Tab::Reading);
    let g = Geo::new(VIEW, 1.0);
    let rows = p.rows(&st, &km);
    let i = rows.iter().position(|r| matches!(r, Row::Stepper { .. })).unwrap();
    let top = p.tops(&rows, &g)[i];
    let right = g.x + g.w - g.pad;
    assert_eq!(
        p.click(&st, &km, VIEW, 1.0, right - 10.0, top + 20.0),
        Handled::Yes(Some(Command::Pref(Pref::Slideshow(6))))
    );
    assert_eq!(
        p.click(&st, &km, VIEW, 1.0, right - 95.0, top + 20.0),
        Handled::Yes(Some(Command::Pref(Pref::Slideshow(4))))
    );
}

#[test]
fn i_tasti_scorrono() {
    let (mut p, st, km) = open(Tab::Keys);
    p.wheel(&st, &km, VIEW, 1.0, 99_999.0);
    let g = Geo::new(VIEW, 1.0);
    let rows = p.rows(&st, &km);
    let last = *p.tops(&rows, &g).last().unwrap();
    assert!(last < VIEW.1 && last > VIEW.1 - 200.0, "in fondo si vede l'ultima riga: {last}");
}
