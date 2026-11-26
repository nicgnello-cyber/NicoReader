//! Thread di lavoro attorno a uno stato condiviso (la coda): chi li possiede
//! cambia lo stato e li sveglia; loro prendono un lavoro, lo fanno, e
//! ricominciano. Quando il proprietario se ne va si fermano, e lo si aspetta.
//! Sono a bassa priorita': chi disegna passa sempre prima (con tutti i core
//! occupati a decodificare, a pari priorita' il nastro scendeva a 22
//! fotogrammi al secondo). Lo usano le pagine, le copertine e l'ingrandimento.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

pub(crate) struct Pool<S> {
    shared: Arc<Shared<S>>,
    workers: Vec<JoinHandle<()>>,
}

pub(crate) struct Shared<S> {
    state: Mutex<Guarded<S>>,
    wake: Condvar,
}

struct Guarded<S> {
    state: S,
    stop: bool,
}

impl<S: Send + 'static> Pool<S> {
    pub(crate) fn new(state: S) -> Pool<S> {
        let shared = Arc::new(Shared { state: Mutex::new(Guarded { state, stop: false }), wake: Condvar::new() });
        Pool { shared, workers: Vec::new() }
    }

    /// Un thread di lavoro, con il suo nome.
    pub(crate) fn spawn(&mut self, name: String, work: impl FnOnce(&Shared<S>) + Send + 'static) {
        let shared = self.shared.clone();
        let worker = std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                let _ = thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min);
                work(&shared)
            })
            .expect("thread di lavoro");
        self.workers.push(worker);
    }

    /// Cambia lo stato e sveglia i thread.
    pub(crate) fn update<R>(&self, change: impl FnOnce(&mut S) -> R) -> R {
        let out = self.shared.with(change);
        self.shared.wake.notify_all();
        out
    }
}

impl<S> Shared<S> {
    fn lock(&self) -> MutexGuard<'_, Guarded<S>> {
        // un thread morto a meta' non deve bloccare gli altri: lo stato resta coerente
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Il prossimo lavoro: si aspetta finche' `take` ne trova uno. `None`:
    /// si chiude, il thread finisce.
    pub(crate) fn next<J>(&self, mut take: impl FnMut(&mut S) -> Option<J>) -> Option<J> {
        let mut g = self.lock();
        loop {
            if g.stop {
                return None;
            }
            if let Some(job) = take(&mut g.state) {
                return Some(job);
            }
            g = self.wake.wait(g).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Guarda o cambia lo stato (per esempio, il lavoro finito), senza
    /// svegliare nessuno.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut S) -> R) -> R {
        f(&mut self.lock().state)
    }

    /// Si sta chiudendo: un lavoro lungo puo' smettere a meta'.
    pub(crate) fn stopping(&self) -> bool {
        self.lock().stop
    }
}

impl<S> Drop for Pool<S> {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::mpsc;

    #[test]
    fn i_lavori_si_fanno_e_ci_si_ferma() {
        let (tx, rx) = mpsc::channel();
        let mut pool = Pool::new(VecDeque::<u32>::new());
        for n in 0..2 {
            let tx = tx.clone();
            pool.spawn(format!("prova-{n}"), move |shared| {
                while let Some(job) = shared.next(VecDeque::pop_front) {
                    let _ = tx.send(job * 10);
                }
            });
        }
        pool.update(|q| q.extend([1, 2, 3]));
        let mut got: Vec<u32> = (0..3).map(|_| rx.recv().unwrap()).collect();
        got.sort_unstable();
        assert_eq!(got, [10, 20, 30]);
        // andandosene, il proprietario ferma i thread e li aspetta
        drop(pool);
        drop(tx);
        assert!(rx.recv().is_err(), "nessuno manda piu' niente");
    }
}
