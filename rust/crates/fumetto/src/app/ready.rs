//! Le pagine vanno sulla scheda video nel thread che le ha preparate, non in
//! quello che disegna: copiare una pagina ingrandita o una per la lente (ai
//! suoi pixel, decine di MB) costava al thread principale fino a 100 ms, e
//! intanto la finestra stava ferma. Finche' la scheda video non e' pronta le
//! pagine arrivano com'erano, e le carica (o le disegna dal processore) chi
//! le riceve.

use std::sync::OnceLock;

use super::*;

/// Cio' che arriva da un thread di lavoro con una pagina da mostrare.
pub(super) trait WithPage: Send + 'static {
    fn page_mut(&mut self) -> Option<&mut Page>;
}

impl WithPage for Loaded {
    fn page_mut(&mut self) -> Option<&mut Page> {
        self.page.as_mut().ok()
    }
}

impl WithPage for Cover {
    fn page_mut(&mut self) -> Option<&mut Page> {
        self.page.as_mut().ok()
    }
}

impl WithPage for Upscaled {
    fn page_mut(&mut self) -> Option<&mut Page> {
        self.page.as_mut()
    }
}

/// Arrivato da un thread di lavoro: con la pagina gia' sulla scheda video,
/// se c'era. Allora i pixel nel processore non ci sono piu' (la pagina ha
/// solo le misure): non servono, e una pagina grande pesa.
pub struct Ready<T> {
    pub(super) item: T,
    pub(super) image: Option<GpuImage>,
}

/// Il disegnatore, per i thread di lavoro: c'e' da quando c'e' la scheda video.
pub(super) type Uploader = Arc<OnceLock<Arc<Renderer>>>;

/// Consegna al thread principale, dopo aver caricato la pagina sulla scheda
/// video se si puo'.
pub(super) fn deliver_ready<T: WithPage>(
    proxy: &EventLoopProxy<UserEvent>, uploader: &Uploader, wrap: fn(Ready<T>) -> UserEvent,
) -> impl Fn(T) + Send + Sync + use<T> {
    let (proxy, uploader) = (proxy.clone(), uploader.clone());
    move |mut item: T| {
        let image = match (uploader.get(), item.page_mut()) {
            (Some(renderer), Some(page)) => renderer.upload(page).ok().inspect(|_| page.rgba = Vec::new()),
            _ => None,
        };
        let _ = proxy.send_event(wrap(Ready { item, image }));
    }
}

/// La pagina sulla scheda video: quella caricata dal thread di lavoro, o
/// caricata adesso (arrivata prima della scheda video, o non riuscita).
/// `None` se la scheda video non c'e' ancora.
pub(super) fn on_gpu(
    renderer: Option<&Renderer>, image: Option<GpuImage>, page: &Page,
) -> Option<Result<GpuImage, String>> {
    match image {
        Some(image) => Some(Ok(image)),
        None => renderer.map(|r| r.upload(page)),
    }
}

impl App {
    /// Il disegnatore, se la scheda video c'e'.
    pub(super) fn renderer(&self) -> Option<&Renderer> {
        self.gfx.as_ref().map(|g| &*g.renderer)
    }
}
