use super::*;

#[test]
fn il_titolo_di_una_cartella_tiene_i_punti() {
    let dir =
        std::env::temp_dir().join(format!("fumetto-test-titolo-{}", std::process::id())).join("Vol.01 Ch.001 - Screw");
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(title_of(&dir), "Vol.01 Ch.001 - Screw");
    assert_eq!(title_of(Path::new("Nebbia sul Porto v03.cbz")), "Nebbia sul Porto v03");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// Tre pagine PNG in una cartella temporanea tutta per il test.
fn pages_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fumetto-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (i, name) in ["p10.png", "p2.png", "p1.png"].iter().enumerate() {
        image::RgbaImage::from_pixel(4, 6, image::Rgba([i as u8 * 50, 0, 0, 255])).save(dir.join(name)).unwrap();
    }
    dir
}

fn check(book: &Book) {
    assert_eq!(book.names, ["p1.png", "p2.png", "p10.png"]);
    let page = crate::decode(&book.read(2).unwrap()).unwrap();
    assert_eq!((page.width, page.height, page.rgba[0]), (4, 6, 0)); // p10 era la prima
}

#[test]
fn cartella_tar_e_7z() {
    let dir = pages_dir("formati");
    check(&Book::open(&dir).unwrap());

    let tar_path = dir.with_extension("cbt");
    let mut builder = tar::Builder::new(File::create(&tar_path).unwrap());
    builder.append_dir_all(".", &dir).unwrap();
    builder.finish().unwrap();
    drop(builder);
    check(&Book::open(&tar_path).unwrap());

    let sevenz_path = dir.with_extension("cb7");
    sevenz_rust2::compress_to_path(&dir, &sevenz_path).unwrap();
    check(&Book::open(&sevenz_path).unwrap());

    for p in [&tar_path, &sevenz_path] {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn la_scheda_in_ogni_formato() {
    let dir = pages_dir("scheda");
    let xml = "<?xml version=\"1.0\"?><ComicInfo><Series>Orbita Bassa</Series><Number>3</Number>\
                   <Manga>YesAndRightToLeft</Manga></ComicInfo>";
    std::fs::write(dir.join("ComicInfo.xml"), xml).unwrap();
    let has_info = |book: &Book| {
        check(book); // la scheda non e' una pagina
        let info = book.info.as_ref().expect("scheda letta");
        assert_eq!((info.series.as_deref(), info.right_to_left), (Some("Orbita Bassa"), Some(true)));
    };
    has_info(&Book::open(&dir).unwrap());

    let tar_path = dir.with_extension("cbt");
    let mut builder = tar::Builder::new(File::create(&tar_path).unwrap());
    builder.append_dir_all(".", &dir).unwrap();
    builder.finish().unwrap();
    drop(builder);
    has_info(&Book::open(&tar_path).unwrap());

    let zip_path = dir.with_extension("cbz");
    let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in ["ComicInfo.xml", "p1.png", "p2.png", "p10.png"] {
        zip.start_file(name, stored).unwrap();
        io::Write::write_all(&mut zip, &std::fs::read(dir.join(name)).unwrap()).unwrap();
    }
    zip.finish().unwrap();
    has_info(&Book::open(&zip_path).unwrap());

    let sevenz_path = dir.with_extension("cb7");
    sevenz_rust2::compress_to_path(&dir, &sevenz_path).unwrap();
    has_info(&Book::open(&sevenz_path).unwrap());

    // senza aprirli tutti: il 7z si dovrebbe decomprimere, si salta
    for p in [&dir, &tar_path, &zip_path] {
        assert_eq!(read_info(p).and_then(|i| i.number).as_deref(), Some("3"), "{}", p.display());
    }
    assert_eq!(read_info(&sevenz_path), None);

    for p in [&tar_path, &zip_path, &sevenz_path] {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn riconosce_le_immagini() {
    assert!(is_image("a/b/p01.JPG"));
    assert!(is_image("p.webp"));
    assert!(!is_image("ComicInfo.xml"));
    assert!(!is_image("__MACOSX/._p01.jpg"));
    assert!(!is_image("jpg"));
}
