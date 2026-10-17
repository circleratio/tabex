//! Background thread pools. Results are sent over a channel and the UI thread is woken with
//! a single coalesced WM_APP_WORKER post.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_APP_WORKER: u32 = WM_APP + 1;

type Job = Box<dyn FnOnce() + Send>;

struct Queue {
    jobs: Mutex<(VecDeque<Job>, bool)>,
    cv: Condvar,
}

/// Fixed-size pool of STA threads (shell objects generally expect an apartment-threaded caller).
pub struct Pool {
    queue: Arc<Queue>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Pool {
    pub fn new(name: &str, n: usize) -> Pool {
        let queue = Arc::new(Queue {
            jobs: Mutex::new((VecDeque::new(), false)),
            cv: Condvar::new(),
        });
        let threads = (0..n)
            .map(|i| {
                let q = queue.clone();
                std::thread::Builder::new()
                    .name(format!("{name}-{i}"))
                    .spawn(move || {
                        unsafe {
                            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                        }
                        loop {
                            let job = {
                                let mut g = q.jobs.lock().unwrap();
                                loop {
                                    if g.1 {
                                        unsafe { CoUninitialize() };
                                        return;
                                    }
                                    if let Some(j) = g.0.pop_front() {
                                        break j;
                                    }
                                    g = q.cv.wait(g).unwrap();
                                }
                            };
                            job();
                        }
                    })
                    .expect("spawn worker thread")
            })
            .collect();
        Pool { queue, threads }
    }

    pub fn spawn(&self, job: impl FnOnce() + Send + 'static) {
        self.queue.jobs.lock().unwrap().0.push_back(Box::new(job));
        self.queue.cv.notify_one();
    }

    /// Drop queued jobs that have not started yet.
    pub fn clear_pending(&self) {
        self.queue.jobs.lock().unwrap().0.clear();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        {
            let mut g = self.queue.jobs.lock().unwrap();
            g.0.clear();
            g.1 = true;
        }
        self.queue.cv.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

#[derive(Clone, Copy)]
struct SendHwnd(HWND);
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

/// Sender half handed to worker jobs.
pub struct Notifier<T> {
    tx: Sender<T>,
    hwnd: SendHwnd,
    posted: Arc<AtomicBool>,
}

impl<T> Clone for Notifier<T> {
    fn clone(&self) -> Self {
        Notifier {
            tx: self.tx.clone(),
            hwnd: self.hwnd,
            posted: self.posted.clone(),
        }
    }
}

impl<T: Send> Notifier<T> {
    pub fn send(&self, msg: T) {
        if self.tx.send(msg).is_ok() && !self.posted.swap(true, Ordering::AcqRel) {
            unsafe {
                let _ = PostMessageW(Some(self.hwnd.0), WM_APP_WORKER, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Receiver half owned by the UI thread.
pub struct Inbox<T> {
    rx: Receiver<T>,
    posted: Arc<AtomicBool>,
}

impl<T> Inbox<T> {
    /// Drain everything received so far. Call when WM_APP_WORKER arrives.
    pub fn drain(&self) -> Vec<T> {
        self.posted.store(false, Ordering::Release);
        self.rx.try_iter().collect()
    }
}

pub fn channel_for<T>(hwnd: HWND) -> (Notifier<T>, Inbox<T>) {
    let (tx, rx) = channel();
    let posted = Arc::new(AtomicBool::new(false));
    (
        Notifier {
            tx,
            hwnd: SendHwnd(hwnd),
            posted: posted.clone(),
        },
        Inbox { rx, posted },
    )
}
